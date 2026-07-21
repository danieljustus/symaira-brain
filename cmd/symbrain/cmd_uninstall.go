package main

import (
	"flag"
	"fmt"
	"io"
	"os"

	"github.com/danieljustus/symaira-brain/internal/harness"
	"github.com/danieljustus/symaira-corekit/exitcodes"
	"github.com/danieljustus/symaira-corekit/fsutil"
)

func cmdUninstall(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	fs := flag.NewFlagSet("uninstall", flag.ContinueOnError)
	harnessName := fs.String("harness", "", "harness to remove symbrain from: claude, claude-desktop, cursor, opencode, codex, gemini (required)")
	projectDir := fs.String("project", "", "project directory; only meaningful for harnesses with a project-local config (currently: claude's .mcp.json)")
	dryRun := fs.Bool("dry-run", false, "print a unified diff of the change and write nothing")
	fs.SetOutput(stderr)
	if err := fs.Parse(args); err != nil {
		return exitcodes.ExitNoInput
	}

	h, path, code := resolveHarnessAndPath(*harnessName, *projectDir, stderr, "uninstall")
	if code != exitcodes.ExitOK {
		return code
	}

	return uninstallFrom(h, path, *dryRun, stdout, stderr)
}

// uninstallFrom removes (or dry-run previews removing) the symbrain MCP
// entry from the config file at path. It only ever touches the entry whose
// command resolves to symbrain — every other server entry in the same file
// is left completely alone.
func uninstallFrom(h harness.Harness, path string, dryRun bool, stdout, stderr io.Writer) exitcodes.ExitCode {
	original, readErr := os.ReadFile(path)
	if os.IsNotExist(readErr) {
		fmt.Fprintf(stdout, "%s: no config file found, nothing to uninstall\n", path)
		return exitcodes.ExitOK
	}
	if readErr != nil {
		fmt.Fprintf(stderr, "symbrain uninstall: read %s: %v\n", path, readErr)
		return exitcodes.ExitGeneric
	}

	doc, err := harness.Parse(h, original)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain uninstall: %s: %s\n", path, exitcodes.FormatCLIError(err))
		return exitcodes.ExitCodeFromError(err)
	}

	entry, present := doc.Server(harness.ServerName)
	if !present || !entry.IsSymbrain() {
		fmt.Fprintf(stdout, "%s: symbrain is not installed, nothing to do\n", path)
		return exitcodes.ExitOK
	}

	doc.RemoveServer(harness.ServerName)
	newContent, err := doc.Marshal()
	if err != nil {
		fmt.Fprintf(stderr, "symbrain uninstall: encode %s: %v\n", path, err)
		return exitcodes.ExitGeneric
	}

	if dryRun {
		fmt.Fprint(stdout, harness.UnifiedDiff(path, original, newContent))
		return exitcodes.ExitOK
	}

	backupPath, err := harness.Backup(path)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain uninstall: back up %s: %v\n", path, err)
		return exitcodes.ExitGeneric
	}
	if backupPath != "" {
		fmt.Fprintf(stdout, "backed up %s to %s\n", path, backupPath)
	}

	if err := fsutil.AtomicWriteFile(path, newContent, 0o600); err != nil {
		fmt.Fprintf(stderr, "symbrain uninstall: write %s: %v\n", path, err)
		return exitcodes.ExitGeneric
	}

	fmt.Fprintf(stdout, "removed symbrain from %s (harness: %s)\n", path, h.Name)
	return exitcodes.ExitOK
}
