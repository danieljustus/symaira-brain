package main

import (
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"

	"github.com/danieljustus/symaira-brain/internal/config"
	"github.com/danieljustus/symaira-brain/internal/harness"
	"github.com/danieljustus/symaira-corekit/exitcodes"
	"github.com/danieljustus/symaira-corekit/fsutil"
)

func cmdInstall(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	fs := flag.NewFlagSet("install", flag.ContinueOnError)
	harnessName := fs.String("harness", "", "harness to install into: claude, claude-desktop, cursor, opencode, codex, gemini (required)")
	profileFlag := fs.String("profile", "", "profile to bind this harness connection to (default: the global config's default_profile)")
	projectDir := fs.String("project", "", "project directory; only meaningful for harnesses with a project-local config (currently: claude's .mcp.json)")
	dryRun := fs.Bool("dry-run", false, "print a unified diff of the change and write nothing")
	fs.SetOutput(stderr)
	if err := fs.Parse(args); err != nil {
		return exitcodes.ExitNoInput
	}

	h, path, code := resolveHarnessAndPath(*harnessName, *projectDir, stderr, "install")
	if code != exitcodes.ExitOK {
		return code
	}

	profile := *profileFlag
	if profile == "" {
		cfg, err := config.Load()
		if err != nil {
			fmt.Fprintf(stderr, "symbrain install: %v\n", err)
			return exitcodes.ExitCodeFromError(err)
		}
		profile = cfg.DefaultProfile
	}
	if profile == "" {
		fmt.Fprintln(stderr, "symbrain install: no --profile given and no default_profile configured in ~/.config/symbrain/config.toml (pass --profile, or run `symbrain init`)")
		return exitcodes.ExitNoInput
	}

	return installInto(h, path, profile, *dryRun, stdout, stderr)
}

// resolveHarnessAndPath looks up the named harness and resolves the config
// file path it should be edited at, honoring --project for harnesses that
// support a project-local config. cmdName is used only for error prefixes
// ("install"/"uninstall").
func resolveHarnessAndPath(harnessName, projectDir string, stderr io.Writer, cmdName string) (harness.Harness, string, exitcodes.ExitCode) {
	if harnessName == "" {
		fmt.Fprintf(stderr, "symbrain %s: --harness is required (want one of: %s)\n", cmdName, joinNames())
		return harness.Harness{}, "", exitcodes.ExitNoInput
	}

	h, err := harness.Lookup(harnessName)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain %s: %s\n", cmdName, exitcodes.FormatCLIError(err))
		return harness.Harness{}, "", exitcodes.ExitCodeFromError(err)
	}

	if projectDir != "" {
		if !h.SupportsProject {
			fmt.Fprintf(stderr, "symbrain %s: harness %q has no project-local config; omit --project\n", cmdName, h.Name)
			return harness.Harness{}, "", exitcodes.ExitNoInput
		}
		return h, h.ProjectConfigPath(projectDir), exitcodes.ExitOK
	}

	path, err := h.ConfigPath()
	if err != nil {
		fmt.Fprintf(stderr, "symbrain %s: resolve config path for %s: %v\n", cmdName, h.Name, err)
		return harness.Harness{}, "", exitcodes.ExitGeneric
	}
	return h, path, exitcodes.ExitOK
}

func joinNames() string {
	names := harness.Names()
	out := ""
	for i, n := range names {
		if i > 0 {
			out += ", "
		}
		out += n
	}
	return out
}

// installInto writes (or dry-run previews) the symbrain MCP entry for
// profile into the config file at path.
func installInto(h harness.Harness, path, profile string, dryRun bool, stdout, stderr io.Writer) exitcodes.ExitCode {
	original, readErr := os.ReadFile(path)
	existed := readErr == nil
	if readErr != nil && !os.IsNotExist(readErr) {
		fmt.Fprintf(stderr, "symbrain install: read %s: %v\n", path, readErr)
		return exitcodes.ExitGeneric
	}

	var doc harness.Document
	if existed {
		var err error
		doc, err = harness.Parse(h, original)
		if err != nil {
			fmt.Fprintf(stderr, "symbrain install: %s: %s\n", path, exitcodes.FormatCLIError(err))
			return exitcodes.ExitCodeFromError(err)
		}
	} else {
		doc = harness.Empty(h)
	}

	doc.SetServer(harness.ServerName, harness.NewEntry(profile))

	newContent, err := doc.Marshal()
	if err != nil {
		fmt.Fprintf(stderr, "symbrain install: encode %s: %v\n", path, err)
		return exitcodes.ExitGeneric
	}

	if dryRun {
		diff := harness.UnifiedDiff(path, original, newContent)
		if diff == "" {
			fmt.Fprintf(stdout, "%s: already up to date, no changes to make\n", path)
			return exitcodes.ExitOK
		}
		fmt.Fprint(stdout, diff)
		return exitcodes.ExitOK
	}

	if existed {
		backupPath, err := harness.Backup(path)
		if err != nil {
			fmt.Fprintf(stderr, "symbrain install: back up %s: %v\n", path, err)
			return exitcodes.ExitGeneric
		}
		if backupPath != "" {
			fmt.Fprintf(stdout, "backed up %s to %s\n", path, backupPath)
		}
	} else if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		fmt.Fprintf(stderr, "symbrain install: create %s: %v\n", filepath.Dir(path), err)
		return exitcodes.ExitGeneric
	}

	if err := fsutil.AtomicWriteFile(path, newContent, 0o600); err != nil {
		fmt.Fprintf(stderr, "symbrain install: write %s: %v\n", path, err)
		return exitcodes.ExitGeneric
	}

	fmt.Fprintf(stdout, "installed symbrain into %s (harness: %s, profile: %s)\n", path, h.Name, profile)
	return exitcodes.ExitOK
}
