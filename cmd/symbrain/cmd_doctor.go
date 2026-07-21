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

	"github.com/danieljustus/symaira-brain/internal/broker"
	"github.com/danieljustus/symaira-brain/internal/config"
	"github.com/danieljustus/symaira-brain/internal/harness"
	"github.com/danieljustus/symaira-brain/internal/policy"
	"github.com/danieljustus/symaira-brain/internal/profile"
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

// harnessCheck reports symbrain's install state in one harness's MCP
// config: whether the config file exists and parses, whether symbrain is
// registered in it, and — narrower than full profile validation, since
// internal/profile does not exist yet (see #8) — whether the profile name
// it's bound to has a corresponding file under
// ~/.config/symbrain/profiles/. Full schema validation against a typed
// profile loader is left for a follow-up once #8 lands.
type harnessCheck struct {
	Name         string `json:"name"`
	ConfigPath   string `json:"config_path"`
	ConfigFound  bool   `json:"config_found"`
	ConfigParsed bool   `json:"config_parsed"`
	ConfigError  string `json:"config_error,omitempty"`
	Installed    bool   `json:"installed"`
	// Profile is the --profile value bound in symbrain's MCP entry, if
	// Installed and the entry carries one.
	Profile string `json:"profile,omitempty"`
	// ProfileExists reports whether ~/.config/symbrain/profiles/<Profile>.toml
	// exists on disk. Only meaningful when Profile is non-empty.
	ProfileExists bool `json:"profile_exists"`
	// ProfileMissing flags a harness bound to a profile that doesn't exist
	// on disk: Installed, Profile is set, and ProfileExists is false.
	ProfileMissing bool `json:"profile_missing"`
}

type doctorReport struct {
	ConfigDir  dirCheck           `json:"config_dir"`
	DataDir    dirCheck           `json:"data_dir"`
	CacheDir   dirCheck           `json:"cache_dir"`
	Config     configCheck        `json:"config"`
	Servers    []serverCheck      `json:"servers"`
	Profiles   []string           `json:"profiles"`
	Harnesses  []harnessCheck     `json:"harnesses"`
	Handshakes []profileHandshake `json:"handshakes,omitempty"`
}

type profileHandshake struct {
	Profile   string `json:"profile"`
	Server    string `json:"server"`
	Protocol  string `json:"protocol_version,omitempty"`
	ToolCount int    `json:"tool_count"`
	Exposed   int    `json:"exposed"`
	Hidden    int    `json:"hidden"`
	Unknown   int    `json:"unknown"`
	Error     string `json:"error,omitempty"`
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
	vaultAgent := fs.String("vault-agent", "claude-code", "vault agent name for MCP handshake probe")
	fs.SetOutput(stderr)
	if err := fs.Parse(args); err != nil {
		return exitcodes.ExitNoInput
	}

	report := runDoctorChecks(context.Background(), *vaultAgent)

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

func runDoctorChecks(ctx context.Context, vaultAgent string) *doctorReport {
	report := &doctorReport{
		ConfigDir: checkDir(xdg.ConfigDir()),
		Config:    checkConfig(),
		Servers:   checkServers(ctx),
		Profiles:  discoverProfiles(),
		Harnesses: checkHarnesses(),
	}

	if dataDir, err := xdg.DataDir(); err == nil {
		report.DataDir = checkDir(dataDir)
	}
	if cacheDir, err := xdg.CacheDir(); err == nil {
		report.CacheDir = checkDir(cacheDir)
	}

	report.Handshakes = checkHandshakes(ctx, vaultAgent)

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

// checkHarnesses inspects every harness in the registry (#19) for whether
// symbrain is installed and, if so, which profile it's bound to.
func checkHarnesses() []harnessCheck {
	checks := make([]harnessCheck, 0, len(harness.All))
	for _, h := range harness.All {
		checks = append(checks, checkHarness(h))
	}
	return checks
}

func checkHarness(h harness.Harness) harnessCheck {
	check := harnessCheck{Name: string(h.Name)}

	path, err := h.ConfigPath()
	if err != nil {
		check.ConfigError = err.Error()
		return check
	}
	check.ConfigPath = path

	data, err := os.ReadFile(path)
	if err != nil {
		if !os.IsNotExist(err) {
			check.ConfigError = err.Error()
		}
		return check
	}
	check.ConfigFound = true

	doc, err := harness.Parse(h, data)
	if err != nil {
		check.ConfigError = exitcodes.FormatCLIError(err)
		return check
	}
	check.ConfigParsed = true

	entry, present := doc.Server(harness.ServerName)
	if !present || !entry.IsSymbrain() {
		return check
	}
	check.Installed = true

	profile, ok := entry.Profile()
	if !ok || profile == "" {
		return check
	}
	check.Profile = profile
	check.ProfileExists = profileFileExists(profile)
	check.ProfileMissing = !check.ProfileExists
	return check
}

// profileFileExists reports whether
// ~/.config/symbrain/profiles/<name>.toml exists on disk. This is the
// narrower check described in issue #21: full schema validation belongs to
// internal/profile (issue #8), which doesn't exist yet on this branch.
func profileFileExists(name string) bool {
	info, err := os.Stat(filepath.Join(xdg.ProfilesDir(), name+".toml"))
	return err == nil && !info.IsDir()
}

const handshakeTimeout = 5 * time.Second

func checkHandshakes(ctx context.Context, vaultAgent string) []profileHandshake {
	names, err := profile.ListNames()
	if err != nil || len(names) == 0 {
		return nil
	}

	cfg, cfgErr := config.Load()
	if cfgErr != nil {
		cfg = config.Defaults()
	}

	var results []profileHandshake
	for _, name := range names {
		p, err := profile.Load(name)
		if err != nil {
			continue
		}

		for _, alias := range []string{profile.ServerVault, profile.ServerMemory, profile.ServerSkills} {
			serverCfg := p.Server(alias)
			if !serverCfg.Enabled {
				continue
			}

			binaryName := aliasBinary(alias)
			override := ""
			switch alias {
			case profile.ServerVault:
				override = cfg.Servers.Vault.BinaryPath
			case profile.ServerMemory:
				override = cfg.Servers.Memory.BinaryPath
			case profile.ServerSkills:
				override = cfg.Servers.Skills.BinaryPath
			}

			path, err := broker.Discover(binaryName, override)
			if err != nil {
				results = append(results, profileHandshake{
					Profile: name,
					Server:  alias,
					Error:   err.Error(),
				})
				continue
			}

			h := probeHandshake(ctx, path, name, alias, vaultAgent, serverCfg)
			results = append(results, h)
		}
	}
	return results
}

func aliasBinary(alias string) string {
	switch alias {
	case profile.ServerVault:
		return "symvault"
	case profile.ServerMemory:
		return "symmemory"
	case profile.ServerSkills:
		return "symskills"
	default:
		return alias
	}
}

func probeHandshake(ctx context.Context, path, profileName, alias, vaultAgent string, serverCfg profile.ServerConfig) profileHandshake {
	h := profileHandshake{Profile: profileName, Server: alias}

	handshakeCtx, cancel := context.WithTimeout(ctx, handshakeTimeout)
	defer cancel()

	args := []string{"serve"}
	if alias == profile.ServerSkills {
		args = []string{"serve", "--stdio"}
	}
	if alias == profile.ServerVault && vaultAgent != "" {
		args = []string{"serve", "--stdio", "--agent", vaultAgent, "--allow-locked"}
	}
	c, err := broker.Spawn(path, broker.Options{Args: args, Stderr: io.Discard})
	if err != nil {
		h.Error = fmt.Sprintf("spawn: %v", err)
		return h
	}
	defer func() {
		_ = c.Close()
		if p := c.Process(); p != nil {
			_ = p.Kill()
		}
	}()

	result, err := c.Initialize(handshakeCtx)
	if err != nil {
		h.Error = fmt.Sprintf("initialize: %v", err)
		return h
	}

	h.Protocol = result.ProtocolVersion

	tools, err := c.ListTools(handshakeCtx)
	if err != nil {
		h.Error = fmt.Sprintf("tools/list: %v", err)
		return h
	}

	h.ToolCount = len(tools)

	liveNames := make([]string, len(tools))
	for i, t := range tools {
		liveNames[i] = t.Name
	}

	report, err := policy.Evaluate(alias, serverCfg, liveNames)
	if err != nil {
		h.Error = fmt.Sprintf("policy: %v", err)
		return h
	}

	h.Exposed = len(report.Exposed)
	h.Hidden = len(report.Hidden)
	h.Unknown = len(report.Unknown)

	return h
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
}

func printHandshake(w io.Writer, h profileHandshake) {
	if h.Error != "" {
		fmt.Fprintf(w, "    ✗  %-10s %-8s handshake failed: %s\n", h.Profile, h.Server, h.Error)
		return
	}
	fmt.Fprintf(w, "    ✓  %-10s %-8s protocol=%s tools=%d exposed=%d hidden=%d unknown=%d\n",
		h.Profile, h.Server, h.Protocol, h.ToolCount, h.Exposed, h.Hidden, h.Unknown)
}
