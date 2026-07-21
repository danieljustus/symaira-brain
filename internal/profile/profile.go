package profile

import (
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/xdg"
	"github.com/danieljustus/symaira-corekit/exitcodes"

	"github.com/BurntSushi/toml"
)

// Server aliases recognized under [servers.*] tables. Any other alias in a
// profile file is a validation error.
const (
	ServerVault  = "vault"
	ServerMemory = "memory"
	ServerSkills = "skills"
)

// Memory server modes.
const (
	MemoryModeReadOnly  = "read_only"
	MemoryModeReadWrite = "read_write"
)

// Vault server modes.
const (
	VaultModeRequestOnly = "request_only"
	VaultModeFull        = "full"
	VaultModeOff         = "off"
)

// Profile is the parsed, validated, defaulted representation of a profile
// TOML file at ~/.config/symbrain/profiles/<name>.toml. One profile
// controls what a single harness connection may see across the three
// state cores (vault, memory, skills).
type Profile struct {
	Name        string `json:"name"`
	Description string `json:"description"`

	Servers Servers     `json:"servers"`
	Audit   AuditConfig `json:"audit"`

	// Warnings holds non-fatal issues found while loading (unknown TOML
	// keys, ignored fields). A non-empty Warnings does not make Load
	// return an error.
	Warnings []string `json:"warnings,omitempty"`
}

// Servers holds the three state-core server configs a profile can shape.
// This is a fixed struct rather than a map — symbrain composes exactly
// these three servers (see internal/config.ServersConfig for the same
// convention on the global config side).
type Servers struct {
	Vault  ServerConfig `json:"vault"`
	Memory ServerConfig `json:"memory"`
	Skills ServerConfig `json:"skills"`
}

// ServerConfig is one [servers.<alias>] table.
type ServerConfig struct {
	Enabled bool `json:"enabled"`
	// Mode selects a named exposure preset (see internal/policy). Only
	// meaningful for vault and memory; skills has no modes and ignores
	// this field (a mode set there is dropped with a warning).
	Mode string `json:"mode,omitempty"`
	// ToolsAllow and ToolsDeny override Mode's preset tool list. The two
	// are mutually combinable; internal/policy resolves them with deny
	// always winning over allow.
	ToolsAllow []string `json:"tools_allow,omitempty"`
	ToolsDeny  []string `json:"tools_deny,omitempty"`
}

// AuditConfig is the [audit] table. Enabled defaults to true.
type AuditConfig struct {
	Enabled bool `json:"enabled"`
}

// fileProfile mirrors the on-disk TOML shape. Servers is decoded as a map
// so unknown aliases (anything other than vault/memory/skills) can be
// detected and rejected explicitly instead of silently accepted.
type fileProfile struct {
	Profile fileProfileMeta       `toml:"profile"`
	Servers map[string]fileServer `toml:"servers"`
	Audit   fileAuditConfig       `toml:"audit"`
}

type fileProfileMeta struct {
	Name        string `toml:"name"`
	Description string `toml:"description"`
}

// fileServer is shared across all three server aliases; not every field is
// meaningful for every alias (skills ignores Mode).
type fileServer struct {
	Enabled    *bool    `toml:"enabled"`
	Mode       string   `toml:"mode"`
	ToolsAllow []string `toml:"tools_allow"`
	ToolsDeny  []string `toml:"tools_deny"`
}

type fileAuditConfig struct {
	Enabled *bool `toml:"enabled"`
}

var knownServerAliases = map[string]bool{
	ServerVault:  true,
	ServerMemory: true,
	ServerSkills: true,
}

// validNamePattern restricts profile names to a safe, unambiguous charset.
// This also closes off path traversal: a name matching this pattern can
// never contain "/", "\", "..", or quote characters, so it is always safe
// to join into a filesystem path (Path) or embed in a TOML string value
// (used by `symbrain profile add` to rewrite a template's [profile] name).
var validNamePattern = regexp.MustCompile(`^[a-zA-Z0-9_-]+$`)

// ValidateName reports whether name is safe to use as a profile: a
// filesystem basename (via Path) and a TOML string value. Callers that
// accept a profile name from user input (CLI args, in particular
// `symbrain profile add`/`remove`) should call this before touching the
// filesystem.
func ValidateName(name string) error {
	if !validNamePattern.MatchString(name) {
		return fmt.Errorf(
			"profile name %q must be non-empty and contain only letters, digits, '-', or '_'", name)
	}
	return nil
}

// Path returns the file path for profile name under xdg.ProfilesDir(),
// without checking whether it exists or whether name is valid.
func Path(name string) string {
	return filepath.Join(xdg.ProfilesDir(), name+".toml")
}

// Exists reports whether a profile file exists for name.
func Exists(name string) bool {
	_, err := os.Stat(Path(name))
	return err == nil
}

// Load reads, parses, and validates the profile named name from
// ~/.config/symbrain/profiles/<name>.toml (via xdg.ProfilesDir()).
//
// Parse and validation failures are returned as *exitcodes.CLIError with
// exitcodes.ExitNoInput / exitcodes.KindConfig, matching internal/config's
// error-handling idiom, so callers can propagate exit code 2.
func Load(name string) (*Profile, error) {
	if err := ValidateName(name); err != nil {
		return nil, exitcodes.Wrap(err, exitcodes.ExitNoInput, exitcodes.KindConfig,
			fmt.Sprintf("profile: invalid name %q", name))
	}

	path := Path(name)
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, exitcodes.Wrap(err, exitcodes.ExitNoInput, exitcodes.KindConfig,
			fmt.Sprintf("profile: failed to read %s", path))
	}
	return parse(name, data)
}

func parse(name string, data []byte) (*Profile, error) {
	var fp fileProfile
	meta, err := toml.Decode(string(data), &fp)
	if err != nil {
		return nil, exitcodes.Wrap(err, exitcodes.ExitNoInput, exitcodes.KindConfig,
			fmt.Sprintf("profile %q: failed to parse TOML", name))
	}

	if fp.Profile.Name != name {
		return nil, exitcodes.Wrap(
			fmt.Errorf("profile.name %q does not match filename %q", fp.Profile.Name, name),
			exitcodes.ExitNoInput, exitcodes.KindConfig,
			fmt.Sprintf("profile %q: name mismatch", name))
	}

	servers, serverWarnings, err := resolveServers(fp.Servers)
	if err != nil {
		return nil, exitcodes.Wrap(err, exitcodes.ExitNoInput, exitcodes.KindConfig,
			fmt.Sprintf("profile %q: invalid servers", name))
	}

	var warnings []string
	for _, key := range meta.Undecoded() {
		warnings = append(warnings, fmt.Sprintf("unknown key %q", key.String()))
	}
	warnings = append(warnings, serverWarnings...)
	sort.Strings(warnings)

	return &Profile{
		Name:        fp.Profile.Name,
		Description: fp.Profile.Description,
		Servers:     servers,
		Audit:       AuditConfig{Enabled: derefBool(fp.Audit.Enabled, true)},
		Warnings:    warnings,
	}, nil
}

func resolveServers(raw map[string]fileServer) (Servers, []string, error) {
	var unknown []string
	for alias := range raw {
		if !knownServerAliases[alias] {
			unknown = append(unknown, alias)
		}
	}
	if len(unknown) > 0 {
		sort.Strings(unknown)
		return Servers{}, nil, fmt.Errorf(
			"unknown server alias(es): %s (must be one of %s, %s, %s)",
			strings.Join(unknown, ", "), ServerVault, ServerMemory, ServerSkills)
	}

	var warnings []string

	vault, err := resolveVault(raw[ServerVault])
	if err != nil {
		return Servers{}, nil, err
	}

	memory, err := resolveMemory(raw[ServerMemory])
	if err != nil {
		return Servers{}, nil, err
	}

	skills, skillsWarnings := resolveSkills(raw[ServerSkills])
	warnings = append(warnings, skillsWarnings...)

	return Servers{Vault: vault, Memory: memory, Skills: skills}, warnings, nil
}

func resolveVault(fs fileServer) (ServerConfig, error) {
	sc := ServerConfig{
		Enabled:    derefBool(fs.Enabled, false),
		ToolsAllow: fs.ToolsAllow,
		ToolsDeny:  fs.ToolsDeny,
	}

	mode := fs.Mode
	if mode == "" {
		// Least-privilege default when a server is enabled but no mode was
		// given explicitly.
		mode = VaultModeRequestOnly
	}
	switch mode {
	case VaultModeRequestOnly, VaultModeFull, VaultModeOff:
		sc.Mode = mode
	default:
		return ServerConfig{}, fmt.Errorf(
			"servers.vault: invalid mode %q (must be one of %s, %s, %s)",
			mode, VaultModeRequestOnly, VaultModeFull, VaultModeOff)
	}
	return sc, nil
}

func resolveMemory(fs fileServer) (ServerConfig, error) {
	sc := ServerConfig{
		Enabled:    derefBool(fs.Enabled, false),
		ToolsAllow: fs.ToolsAllow,
		ToolsDeny:  fs.ToolsDeny,
	}

	mode := fs.Mode
	if mode == "" {
		mode = MemoryModeReadOnly
	}
	switch mode {
	case MemoryModeReadOnly, MemoryModeReadWrite:
		sc.Mode = mode
	default:
		return ServerConfig{}, fmt.Errorf(
			"servers.memory: invalid mode %q (must be one of %s, %s)",
			mode, MemoryModeReadOnly, MemoryModeReadWrite)
	}
	return sc, nil
}

// resolveSkills never errors: skills has no modes in the spec, just
// enabled/disabled plus optional tools_allow/tools_deny narrowing.
func resolveSkills(fs fileServer) (ServerConfig, []string) {
	sc := ServerConfig{
		Enabled:    derefBool(fs.Enabled, false),
		ToolsAllow: fs.ToolsAllow,
		ToolsDeny:  fs.ToolsDeny,
	}

	var warnings []string
	if fs.Mode != "" {
		warnings = append(warnings, fmt.Sprintf(
			"servers.skills: mode %q is ignored (skills has no modes)", fs.Mode))
	}
	return sc, warnings
}

func derefBool(p *bool, fallback bool) bool {
	if p == nil {
		return fallback
	}
	return *p
}

// ListNames returns the sorted profile names found under xdg.ProfilesDir()
// (file basenames without ".toml"). It only lists the directory — it does
// not parse or validate each file; use Load or LoadAll for that. A missing
// profiles directory is not an error: it yields an empty slice.
func ListNames() ([]string, error) {
	entries, err := os.ReadDir(xdg.ProfilesDir())
	if err != nil {
		if os.IsNotExist(err) {
			return []string{}, nil
		}
		return nil, exitcodes.Wrap(err, exitcodes.ExitNoInput, exitcodes.KindConfig,
			"profile: failed to list profiles directory")
	}

	names := make([]string, 0, len(entries))
	for _, e := range entries {
		if e.IsDir() || filepath.Ext(e.Name()) != ".toml" {
			continue
		}
		names = append(names, strings.TrimSuffix(e.Name(), ".toml"))
	}
	sort.Strings(names)
	return names, nil
}

// LoadResult pairs a profile name with the outcome of loading it, so a
// caller (e.g. `symbrain profile list`) can report partial success instead
// of failing outright because one profile file is broken.
type LoadResult struct {
	Name    string
	Profile *Profile
	Err     error
}

// LoadAll loads and validates every profile found by ListNames. It only
// returns a top-level error if the profiles directory itself could not be
// listed; per-file failures are reported in each LoadResult.Err.
func LoadAll() ([]LoadResult, error) {
	names, err := ListNames()
	if err != nil {
		return nil, err
	}

	results := make([]LoadResult, 0, len(names))
	for _, name := range names {
		p, err := Load(name)
		results = append(results, LoadResult{Name: name, Profile: p, Err: err})
	}
	return results, nil
}
