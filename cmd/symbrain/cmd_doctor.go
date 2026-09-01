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
	"runtime"
	"sort"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/audit"
	"github.com/danieljustus/symaira-brain/internal/config"
	"github.com/danieljustus/symaira-brain/internal/harness"
	"github.com/danieljustus/symaira-brain/internal/managed"
	"github.com/danieljustus/symaira-brain/internal/output"
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
	Name           string `json:"name"`
	Binary         string `json:"binary"`
	Found          bool   `json:"found"`
	Path           string `json:"path,omitempty"`
	Version        string `json:"version,omitempty"`
	ManagedVersion string `json:"managed_version,omitempty"`
	Origin         string `json:"origin,omitempty"` // "managed", "path", or "override"
	ProbeError     string `json:"probe_error,omitempty"`
	InstallHint    string `json:"install_hint,omitempty"`
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
	// SupportsMCPInstall reports whether symbrain can register its MCP
	// entry in this harness at all. A capability-only harness (skills or
	// instructions but no MCP config) is never "installed" and must not be
	// presented as merely uninstalled — consumers filter on this rather
	// than hardcoding a list of installable harnesses.
	SupportsMCPInstall bool `json:"supports_mcp_install"`
	Installed          bool `json:"installed"`
	// Profile is the --profile value bound in symbrain's MCP entry, if
	// Installed and the entry carries one.
	Profile string `json:"profile,omitempty"`
	// ProfileExists reports whether ~/.config/symbrain/profiles/<Profile>.toml
	// exists on disk. Only meaningful when Profile is non-empty.
	ProfileExists bool `json:"profile_exists"`
	// ProfileMissing flags a harness bound to a profile that doesn't exist
	// on disk: Installed, Profile is set, and ProfileExists is false.
	ProfileMissing bool `json:"profile_missing"`
	// Superseded lists MCP entries in this harness config that point at a
	// superseded standalone core (symmemory/symskills) symbrain now serves
	// in-process. A harness where Installed && len(Superseded) > 0 runs the
	// gateway and the raw core side by side — a live tool collision, not
	// just untidiness (issue #337).
	Superseded []string `json:"superseded,omitempty"`
}

type doctorReport struct {
	ConfigDir    dirCheck            `json:"config_dir"`
	DataDir      dirCheck            `json:"data_dir"`
	CacheDir     dirCheck            `json:"cache_dir"`
	Config       configCheck         `json:"config"`
	ManagedDir   dirCheck            `json:"managed_dir"`
	Builtins     []string            `json:"builtins"`
	Servers      []serverCheck       `json:"servers"`
	ManagedCores []managedCoreCheck  `json:"managed_cores,omitempty"`
	Profiles     []string            `json:"profiles"`
	Harnesses    []harnessCheck      `json:"harnesses"`
	Handshakes   []profileHandshake  `json:"handshakes,omitempty"`
	Degradations []audit.Degradation `json:"degradations,omitempty"`
}

// managedCoreCheck reports one managed runtime core's pinned and installed
// version. Cores restricted to another platform are omitted entirely.
type managedCoreCheck struct {
	Name    string `json:"name"`
	Pinned  string `json:"pinned"`
	Version string `json:"version,omitempty"`
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

// knownServers are the state cores symbrain still composes out of process.
// A missing binary is a warning (with an install hint), never an error — see
// AGENTS.md "Standalone-First".
//
// Only vault is on this list. Memory and skills were absorbed into this
// binary by the repo consolidation (step 4) and are served from
// internal/memory and internal/skills, so probing for sibling binaries
// would report on tools that are no longer supposed to exist — and their
// Homebrew formulae are deprecated, which made the install hints point
// nowhere. Vault deliberately stays a separate process: the secret store is
// built on the assumption that its caller is untrusted, and that boundary is
// the security mechanism (repo-konsolidierung.md §4).
var knownServers = []struct {
	name        string
	binary      string
	installHint string
}{
	{"vault", "symvault", "brew install danieljustus/tap/symvault"},
}

// builtinServers are the state cores and embedded tools that ship inside
// this binary. Doctor lists them so the report answers "is memory
// available?" rather than "is the sibling binary present?" (usage is the
// gateway-owned usage tool — embedded like memory and skills).
var builtinServers = []string{"memory", "skills", "usage"}

func cmdDoctor(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	format, args, err := extractFormat(args)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain doctor: %v\n", err)
		return exitcodes.ExitNoInput
	}
	return cmdDoctorWithFormat(args, stdout, stderr, format)
}

func cmdDoctorWithFormat(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	fs := flag.NewFlagSet("doctor", flag.ContinueOnError)
	jsonOut := fs.Bool("json", false, "emit machine-readable JSON")
	fix := fs.Bool("fix", false, "repair missing or version-mismatched managed binaries")
	vaultAgent := fs.String("vault-agent", "claude-code", "vault agent name for MCP handshake probe")
	fs.SetOutput(stderr)
	if err := fs.Parse(normalizeFlags(args)); err != nil {
		return exitcodes.ExitNoInput
	}

	if *fix {
		return runDoctorFix(stdout, stderr)
	}

	report := runDoctorChecks(context.Background(), *vaultAgent)

	if *jsonOut || format == output.FormatJSON {
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
		Builtins:  builtinServers,
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
	if managedDir, err := xdg.ManagedBinDir(); err == nil {
		report.ManagedDir = checkDir(managedDir)
		report.ManagedCores = managedCoreChecks(ctx, managedDir)
	}

	report.Handshakes = checkHandshakes(ctx, vaultAgent)
	if degradations, err := audit.LatestDegradations(""); err != nil {
		report.Degradations = []audit.Degradation{{
			Server: "audit",
			Reason: fmt.Sprintf("read audit log: %v", err),
			Level:  "warning",
		}}
	} else {
		report.Degradations = degradations
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

	// Determine managed directory for version reporting
	managedDir := ""
	if dir, err := xdg.ManagedBinDir(); err == nil {
		managedDir = dir
	}

	for _, s := range knownServers {
		check := serverCheck{Name: s.name, Binary: s.binary}

		// Check managed version first
		if managedDir != "" {
			if v, err := managed.InstalledVersion(ctx, managedDir, s.binary); err == nil && v != "" {
				check.ManagedVersion = v
			}
		}

		path, err := exec.LookPath(s.binary)
		if err != nil {
			check.InstallHint = s.installHint
			if check.ManagedVersion != "" {
				check.Found = true
				check.Path = filepath.Join(managedDir, s.binary)
				check.Origin = "managed"
				check.Version = check.ManagedVersion
			}
			checks = append(checks, check)
			continue
		}
		check.Found = true
		check.Path = path

		// Determine origin
		if managedDir != "" && strings.HasPrefix(path, managedDir) {
			check.Origin = "managed"
		} else {
			check.Origin = "path"
		}

		if version, err := probeVersion(ctx, path); err != nil {
			check.ProbeError = err.Error()
		} else {
			check.Version = version
		}
		checks = append(checks, check)
	}
	return checks
}

// managedCoreChecks reports the install state of every managed runtime
// core that ships for this platform. It complements the state-core server
// checks: symcockpit is a managed binary but not a state core, so it
// appears here (and only on macOS) rather than in the Servers section.
//
// The manifest-load error branch (lines 293-296) is unreachable in
// practice: LoadManifest reads an embedded manifest.json that is
// validated at compile time by go:embed; a corrupt embedded file would
// fail to build. It is kept as defensive programming.
func managedCoreChecks(ctx context.Context, binDir string) []managedCoreCheck {
	manifest, err := managed.LoadManifest()
	if err != nil {
		return nil
	}
	checks := make([]managedCoreCheck, 0, len(manifest.Cores))
	for name, core := range manifest.Cores {
		if !core.SupportsPlatform(runtime.GOOS) {
			continue
		}
		c := managedCoreCheck{Name: name, Pinned: core.Version}
		if v, err := managed.InstalledVersion(ctx, binDir, core.BinaryName); err == nil && v != "" {
			c.Version = v
		}
		checks = append(checks, c)
	}
	sort.Slice(checks, func(i, j int) bool { return checks[i].Name < checks[j].Name })
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
	check := harnessCheck{Name: string(h.Name), SupportsMCPInstall: h.SupportsMCPInstall}

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

	// Report superseded core entries (symmemory/symskills) registered in
	// this harness config. They duplicate the in-process gateway tools.
	for _, name := range doc.ServerNames() {
		if entry, ok := doc.Server(name); ok {
			if core, isSuperseded := entry.SupersededCore(); isSuperseded {
				check.Superseded = append(check.Superseded, fmt.Sprintf("%s (superseded %s)", name, core))
			}
		}
	}

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

// runDoctorFix repairs missing or version-mismatched managed binaries.
func runDoctorFix(stdout, stderr io.Writer) exitcodes.ExitCode {
	binDir, err := xdg.ManagedBinDir()
	if err != nil {
		fmt.Fprintf(stderr, "symbrain doctor --fix: %v\n", err)
		return exitcodes.ExitGeneric
	}

	fmt.Fprintf(stdout, "symbrain doctor --fix\n\n")

	if err := managed.Fix(context.Background(), binDir, nil); err != nil {
		fmt.Fprintf(stderr, "  ✗  repair failed: %v\n", err)
		return exitcodes.ExitGeneric
	}

	fmt.Fprintf(stdout, "\nDone. Binaries installed to %s\n", binDir)
	return exitcodes.ExitOK
}
