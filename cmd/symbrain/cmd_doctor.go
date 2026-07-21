package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/config"
	"github.com/danieljustus/symaira-brain/internal/xdg"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// probeTimeout bounds how long doctor waits for a child's `version --json`
// probe before treating it as a (non-fatal) probe failure.
const probeTimeout = 3 * time.Second

type dirCheck struct {
	Path   string `json:"path"`
	Exists bool   `json:"exists"`
}

type configCheck struct {
	Path   string `json:"path"`
	Exists bool   `json:"exists"`
	Parsed bool   `json:"parsed"`
	Error  string `json:"error,omitempty"`
}

type serverCheck struct {
	Name        string `json:"name"`
	Binary      string `json:"binary"`
	Found       bool   `json:"found"`
	Path        string `json:"path,omitempty"`
	Version     string `json:"version,omitempty"`
	ProbeError  string `json:"probe_error,omitempty"`
	InstallHint string `json:"install_hint,omitempty"`
}

type doctorReport struct {
	ConfigDir dirCheck      `json:"config_dir"`
	DataDir   dirCheck      `json:"data_dir"`
	CacheDir  dirCheck      `json:"cache_dir"`
	Config    configCheck   `json:"config"`
	Servers   []serverCheck `json:"servers"`
	Profiles  []string      `json:"profiles"`
}

// knownServers are the three state cores symbrain composes. A missing
// binary is a warning (with an install hint), never an error — see
// AGENTS.md "Standalone-First".
var knownServers = []struct {
	name        string
	binary      string
	installHint string
}{
	{"vault", "symvault", "brew install danieljustus/tap/symvault"},
	{"memory", "symmemory", "brew install danieljustus/tap/symmemory"},
	{"skills", "symskills", "brew install danieljustus/tap/symskills"},
}

func cmdDoctor(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	fs := flag.NewFlagSet("doctor", flag.ContinueOnError)
	jsonOut := fs.Bool("json", false, "emit machine-readable JSON")
	fs.SetOutput(stderr)
	if err := fs.Parse(args); err != nil {
		return exitcodes.ExitNoInput
	}

	report := runDoctorChecks(context.Background())

	if *jsonOut {
		if err := json.NewEncoder(stdout).Encode(report); err != nil {
			fmt.Fprintf(stderr, "symbrain doctor: %v\n", err)
			return exitcodes.ExitGeneric
		}
		return exitcodes.ExitOK
	}

	printDoctorHuman(stdout, report)
	// Doctor only diagnoses; a degraded (but non-fatal) environment is
	// explained above, never turned into a failing exit code.
	return exitcodes.ExitOK
}

func runDoctorChecks(ctx context.Context) *doctorReport {
	report := &doctorReport{
		ConfigDir: checkDir(xdg.ConfigDir()),
		Config:    checkConfig(),
		Servers:   checkServers(ctx),
		Profiles:  discoverProfiles(),
	}

	if dataDir, err := xdg.DataDir(); err == nil {
		report.DataDir = checkDir(dataDir)
	}
	if cacheDir, err := xdg.CacheDir(); err == nil {
		report.CacheDir = checkDir(cacheDir)
	}

	return report
}

func checkDir(path string) dirCheck {
	info, err := os.Stat(path)
	return dirCheck{Path: path, Exists: err == nil && info.IsDir()}
}

func checkConfig() configCheck {
	path := xdg.ConfigPath()
	_, statErr := os.Stat(path)
	c := configCheck{Path: path, Exists: statErr == nil}

	if _, err := config.Load(); err != nil {
		c.Error = exitcodes.FormatCLIError(err)
	} else {
		c.Parsed = true
	}
	return c
}

func checkServers(ctx context.Context) []serverCheck {
	checks := make([]serverCheck, 0, len(knownServers))
	for _, s := range knownServers {
		check := serverCheck{Name: s.name, Binary: s.binary}

		path, err := exec.LookPath(s.binary)
		if err != nil {
			check.InstallHint = s.installHint
			checks = append(checks, check)
			continue
		}
		check.Found = true
		check.Path = path

		if version, err := probeVersion(ctx, path); err != nil {
			check.ProbeError = err.Error()
		} else {
			check.Version = version
		}
		checks = append(checks, check)
	}
	return checks
}

// probeVersion runs `<path> version --json` and extracts the versionkit
// "version" field. Any failure (missing subcommand, timeout, invalid JSON)
// is returned as an error for the caller to record as a non-fatal warning.
func probeVersion(ctx context.Context, path string) (string, error) {
	ctx, cancel := context.WithTimeout(ctx, probeTimeout)
	defer cancel()

	out, err := exec.CommandContext(ctx, path, "version", "--json").Output()
	if err != nil {
		return "", fmt.Errorf("run %s version --json: %w", path, err)
	}

	var payload struct {
		Version string `json:"version"`
	}
	if err := json.Unmarshal(out, &payload); err != nil {
		return "", fmt.Errorf("parse version --json output: %w", err)
	}
	return payload.Version, nil
}

// discoverProfiles lists the profile names under ~/.config/symbrain/profiles
// (file basenames without the .toml extension). Schema validation is
// internal/profile's job once #8 lands; this only reports what's there.
func discoverProfiles() []string {
	entries, err := os.ReadDir(xdg.ProfilesDir())
	if err != nil {
		return []string{}
	}

	names := make([]string, 0, len(entries))
	for _, e := range entries {
		if e.IsDir() || filepath.Ext(e.Name()) != ".toml" {
			continue
		}
		names = append(names, strings.TrimSuffix(e.Name(), ".toml"))
	}
	sort.Strings(names)
	return names
}

func printDoctorHuman(w io.Writer, r *doctorReport) {
	fmt.Fprintln(w, "symbrain doctor")
	fmt.Fprintln(w)

	printDir(w, "config dir", r.ConfigDir)
	printDir(w, "data dir", r.DataDir)
	printDir(w, "cache dir", r.CacheDir)
	printConfig(w, r.Config)

	fmt.Fprintln(w)
	for _, s := range r.Servers {
		printServer(w, s)
	}

	fmt.Fprintln(w)
	if len(r.Profiles) == 0 {
		fmt.Fprintln(w, "  →  no profiles found (run `symbrain init` for examples)")
		return
	}
	fmt.Fprintf(w, "  ✓  profiles: %s\n", strings.Join(r.Profiles, ", "))
}

func printDir(w io.Writer, label string, d dirCheck) {
	mark := "✗"
	if d.Exists {
		mark = "✓"
	}
	fmt.Fprintf(w, "  %s  %-12s %s\n", mark, label, d.Path)
}

func printConfig(w io.Writer, c configCheck) {
	switch {
	case !c.Exists:
		fmt.Fprintf(w, "  →  %-12s not found (run `symbrain init`)\n", "config.toml")
	case c.Parsed:
		fmt.Fprintf(w, "  ✓  %-12s %s\n", "config.toml", c.Path)
	default:
		fmt.Fprintf(w, "  ✗  %-12s %s: %s\n", "config.toml", c.Path, c.Error)
	}
}

func printServer(w io.Writer, s serverCheck) {
	switch {
	case s.Found && s.ProbeError == "":
		fmt.Fprintf(w, "  ✓  %-8s %s (%s)\n", s.Name, s.Path, s.Version)
	case s.Found:
		fmt.Fprintf(w, "  ✗  %-8s %s (version probe failed: %s)\n", s.Name, s.Path, s.ProbeError)
	default:
		fmt.Fprintf(w, "  →  %-8s not found on PATH — %s\n", s.Name, s.InstallHint)
	}
}
