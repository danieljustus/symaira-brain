package main

import (
	"fmt"
	"io"
	"strings"
)

// Human-readable output helpers for `symbrain doctor`. They are split
// from cmd_doctor.go to keep each file under the 400-line rule; there is
// no behavior difference.

func printDoctorHuman(w io.Writer, r *doctorReport) {
	fmt.Fprintln(w, "symbrain doctor")
	fmt.Fprintln(w)

	printDir(w, "config dir", r.ConfigDir)
	printDir(w, "data dir", r.DataDir)
	printDir(w, "cache dir", r.CacheDir)
	printDir(w, "managed dir", r.ManagedDir)
	printConfig(w, r.Config)

	fmt.Fprintln(w)
	for _, name := range r.Builtins {
		fmt.Fprintf(w, "  ✓  %-8s built in\n", name)
	}
	for _, s := range r.Servers {
		printServer(w, s)
	}

	if len(r.ManagedCores) > 0 {
		fmt.Fprintln(w, "  managed cores:")
		for _, c := range r.ManagedCores {
			if c.Version != "" {
				fmt.Fprintf(w, "    ✓  %-10s %s (pinned %s)\n", c.Name, c.Version, c.Pinned)
			} else {
				fmt.Fprintf(w, "    →  %-10s not installed (pinned %s) — run `symbrain doctor --fix`\n", c.Name, c.Pinned)
			}
		}
	}

	fmt.Fprintln(w)
	if len(r.Profiles) == 0 {
		fmt.Fprintln(w, "  →  no profiles found (run `symbrain init` for examples)")
	} else {
		fmt.Fprintf(w, "  ✓  profiles: %s\n", strings.Join(r.Profiles, ", "))
	}

	fmt.Fprintln(w)
	for _, h := range r.Harnesses {
		printHarness(w, h)
	}

	if len(r.Handshakes) > 0 {
		fmt.Fprintln(w)
		fmt.Fprintln(w, "  profile handshakes:")
		for _, h := range r.Handshakes {
			printHandshake(w, h)
		}
	}
	if len(r.Degradations) > 0 {
		fmt.Fprintln(w)
		fmt.Fprintln(w, "  startup degradations:")
		for _, d := range r.Degradations {
			fmt.Fprintf(w, "    !  %-8s %s: %s\n", d.Server, d.Level, d.Reason)
		}
	}
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
	origin := ""
	if s.Origin != "" {
		origin = " [" + s.Origin + "]"
	}
	switch {
	case s.Found && s.ProbeError == "":
		fmt.Fprintf(w, "  ✓  %-8s %s (%s)%s\n", s.Name, s.Path, s.Version, origin)
	case s.Found:
		fmt.Fprintf(w, "  ✗  %-8s %s (version probe failed: %s)%s\n", s.Name, s.Path, s.ProbeError, origin)
	default:
		if s.ManagedVersion != "" {
			fmt.Fprintf(w, "  →  %-8s managed v%s (not on PATH)\n", s.Name, s.ManagedVersion)
		} else {
			fmt.Fprintf(w, "  →  %-8s not found on PATH — %s\n", s.Name, s.InstallHint)
		}
	}
}

func printHarness(w io.Writer, h harnessCheck) {
	switch {
	case !h.ConfigFound:
		fmt.Fprintf(w, "  →  %-14s config not found: %s\n", h.Name, h.ConfigPath)
	case !h.ConfigParsed:
		fmt.Fprintf(w, "  ✗  %-14s %s (invalid config: %s)\n", h.Name, h.ConfigPath, h.ConfigError)
	case !h.Installed:
		fmt.Fprintf(w, "  →  %-14s config found, symbrain not installed: %s\n", h.Name, h.ConfigPath)
	case h.Profile == "":
		fmt.Fprintf(w, "  ✗  %-14s installed but no --profile bound: %s\n", h.Name, h.ConfigPath)
	case h.ProfileMissing:
		fmt.Fprintf(w, "  ✗  %-14s installed, bound to missing profile %q: %s\n", h.Name, h.Profile, h.ConfigPath)
	default:
		fmt.Fprintf(w, "  ✓  %-14s installed, profile %q: %s\n", h.Name, h.Profile, h.ConfigPath)
	}

	// Side-by-side superseded core entries are a live tool collision, not
	// just untidiness (issue #337): the harness exposes both the gateway
	// and the raw symmemory/symskills core, with the core losing on
	// tool-name collisions. `symbrain install` migrates them out.
	if len(h.Superseded) > 0 {
		joined := strings.Join(h.Superseded, ", ")
		if h.Installed {
			fmt.Fprintf(w, "  !  %-14s superseded core entries registered beside symbrain: %s (run `symbrain install` to migrate)\n", h.Name, joined)
		} else {
			fmt.Fprintf(w, "  →  %-14s superseded core entries present (no symbrain): %s\n", h.Name, joined)
		}
	}
}

func printHandshake(w io.Writer, h profileHandshake) {
	if h.Error != "" {
		fmt.Fprintf(w, "    ✗  %-10s %-8s handshake failed: %s\n", h.Profile, h.Server, h.Error)
		return
	}
	fmt.Fprintf(w, "    ✓  %-10s %-8s protocol=%s tools=%d exposed=%d hidden=%d unknown=%d\n",
		h.Profile, h.Server, h.Protocol, h.ToolCount, h.Exposed, h.Hidden, h.Unknown)
}
