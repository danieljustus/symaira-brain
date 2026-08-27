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
	ServerUsage  = "usage"
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

// Servers maps server aliases to their exposure configs. The four reserved
// core aliases (vault, memory, skills, usage) are always present, resolved
// with their mode presets and default-deny behavior. Any additional alias in
// a profile file is a foreign server: it carries no mode preset and instead
// requires a command (with optional args) or a url. (ADR 0001, D2/D4)
type Servers map[string]ServerConfig

// ServerConfig is one [servers.<alias>] table.
type ServerConfig struct {
	Enabled bool `json:"enabled"`
	// Mode selects a named exposure preset (see internal/policy). Only
	// meaningful for vault and memory; skills has no modes and ignores
	// this field (a mode set there is dropped with a warning). Foreign
	// servers never carry a mode — their exposure is read/write classified
	// per profile (issue #335).
	Mode string `json:"mode,omitempty"`
	// ToolsAllow and ToolsDeny override Mode's preset tool list. The two
	// are mutually combinable; internal/policy resolves them with deny
	// always winning over allow.
	ToolsAllow []string `json:"tools_allow,omitempty"`
	ToolsDeny  []string `json:"tools_deny,omitempty"`
	// Command, Args and URL express the transport of a foreign server.
	// A foreign server must set command (with optional args) or url.
	// Core aliases must not set them (they are not foreign servers).
	Command string   `json:"command,omitempty"`
	Args    []string `json:"args,omitempty"`
	URL     string   `json:"url,omitempty"`
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

// fileServer is shared across all server aliases; not every field is
// meaningful for every alias (skills ignores Mode, foreign servers have no
// Mode at all and must carry command/args or url).
type fileServer struct {
	Enabled    *bool    `toml:"enabled"`
	Mode       string   `toml:"mode"`
	ToolsAllow []string `toml:"tools_allow"`
	ToolsDeny  []string `toml:"tools_deny"`
	Command    string   `toml:"command"`
	Args       []string `toml:"args"`
	URL        string   `toml:"url"`
}

type fileAuditConfig struct {
	Enabled *bool `toml:"enabled"`
}

var knownServerAliases = map[string]bool{
	ServerVault:  true,
	ServerMemory: true,
	ServerSkills: true,
	ServerUsage:  true,
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

// LoadFile reads, parses, and validates the profile at path. Unlike Load,
// the profile name comes from the file's own [profile] name field, so the
// file may live anywhere on disk — e.g. next to a room that carries its
// agent profiles with it (see issue #189). The name is still validated
// against the same safe charset, since it is used for the audit log and
// instructions.
func LoadFile(path string) (*Profile, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, exitcodes.Wrap(err, exitcodes.ExitNoInput, exitcodes.KindConfig,
			fmt.Sprintf("profile: failed to read %s", path))
	}

	// Decode just the meta table to learn the profile's name, then run the
	// full parse (which re-decodes and validates everything, including the
	// name match).
	var meta struct {
		Profile fileProfileMeta `toml:"profile"`
	}
	if _, err := toml.Decode(string(data), &meta); err != nil {
		return nil, exitcodes.Wrap(err, exitcodes.ExitNoInput, exitcodes.KindConfig,
			fmt.Sprintf("profile: failed to parse TOML in %s", path))
	}
	name := meta.Profile.Name
	if err := ValidateName(name); err != nil {
		return nil, exitcodes.Wrap(err, exitcodes.ExitNoInput, exitcodes.KindConfig,
			fmt.Sprintf("profile %q: invalid or missing name in %s", name, path))
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
	servers := make(Servers, len(raw)+len(knownServerAliases))
	var warnings []string

	// A core alias redefined as a foreign server (command/args/url set) is
	// a collision: the four cores are reserved and always resolved as such.
	for alias, fs := range raw {
		if !knownServerAliases[alias] {
			continue
		}
		if fs.Command != "" || len(fs.Args) > 0 || fs.URL != "" {
			return nil, nil, fmt.Errorf(
				"servers.%s: core server cannot carry command/args/url — it is not a foreign server", alias)
		}
	}

	// The four core aliases are always present, resolved with their mode
	// presets and default-deny behavior (absent from the file = disabled).
	coreOrder := []string{ServerVault, ServerMemory, ServerSkills, ServerUsage}
	for _, alias := range coreOrder {
		fs := raw[alias]
		switch alias {
		case ServerVault:
			sc, err := resolveVault(fs)
			if err != nil {
				return nil, nil, err
			}
			servers[alias] = sc
		case ServerMemory:
			sc, err := resolveMemory(fs)
			if err != nil {
				return nil, nil, err
			}
			servers[alias] = sc
		case ServerSkills:
			sc, sw := resolveSkills(fs)
			warnings = append(warnings, sw...)
			servers[alias] = sc
		case ServerUsage:
			sc, uw := resolveUsage(fs)
			warnings = append(warnings, uw...)
			servers[alias] = sc
		}
	}

	// Anything else is a foreign server: it needs a transport and carries
	// no mode preset.
	var foreign []string
	for alias := range raw {
		if !knownServerAliases[alias] {
			foreign = append(foreign, alias)
		}
	}
	sort.Strings(foreign)
	for _, alias := range foreign {
		sc, warningsAdded, err := resolveForeign(alias, raw[alias])
		if err != nil {
			return nil, nil, err
		}
		warnings = append(warnings, warningsAdded...)
		servers[alias] = sc
	}

	return servers, warnings, nil
}

// resolveForeign validates and resolves a server outside the four cores. A
// foreign server has no mode preset — its exposure is read/write classified
// per profile (issue #335) — and must declare a transport: command (with
// optional args) or url.
func resolveForeign(alias string, fs fileServer) (ServerConfig, []string, error) {
	if fs.Command == "" && fs.URL == "" {
		return ServerConfig{}, nil, fmt.Errorf(
			"servers.%s: foreign server requires command (with optional args) or url", alias)
	}

	var warnings []string
	if fs.Mode != "" {
		warnings = append(warnings, fmt.Sprintf(
			"servers.%s: mode %q is ignored (foreign servers have no mode presets)", alias, fs.Mode))
	}

	return ServerConfig{
		Enabled:    derefBool(fs.Enabled, false),
		Command:    fs.Command,
		Args:       fs.Args,
		URL:        fs.URL,
		ToolsAllow: fs.ToolsAllow,
		ToolsDeny:  fs.ToolsDeny,
	}, warnings, nil
}

// IsCoreAlias reports whether alias is one of the four reserved core servers
// (vault, memory, skills, usage). Foreign servers are every other alias.
func IsCoreAlias(alias string) bool {
	return knownServerAliases[alias]
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

// resolveUsage never errors: usage has no modes in the spec, just
// enabled/disabled plus optional tools_allow/tools_deny narrowing (the
// single usage tool is get_ai_usage).
func resolveUsage(fs fileServer) (ServerConfig, []string) {
	sc := ServerConfig{
		Enabled:    derefBool(fs.Enabled, false),
		ToolsAllow: fs.ToolsAllow,
		ToolsDeny:  fs.ToolsDeny,
	}

	var warnings []string
	if fs.Mode != "" {
		warnings = append(warnings, fmt.Sprintf(
			"servers.usage: mode %q is ignored (usage has no modes)", fs.Mode))
	}
	return sc, warnings
}

func derefBool(p *bool, fallback bool) bool {
	if p == nil {
		return fallback
	}
	return *p
}

// Server returns the ServerConfig for the given alias (e.g. "vault",
// "memory", "skills", "usage", or a foreign server alias). Returns a
// zero-value ServerConfig for an alias absent from the profile.
func (p *Profile) Server(alias string) ServerConfig {
	if p.Servers == nil {
		return ServerConfig{}
	}
	return p.Servers[alias]
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
