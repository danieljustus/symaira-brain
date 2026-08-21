// Package config defines the TOML configuration schema for symguard and
// provides a loader that resolves XDG Base Directory paths with environment
// variable overrides.
//
// The configuration file lives at:
//
//	$XDG_CONFIG_HOME/symguard/config.toml
//
// with a fallback to ~/.config/symguard/config.toml when XDG_CONFIG_HOME is
// unset. The SYMGUARD_CONFIG environment variable overrides both.
//
// # Schema evolution
//
// This schema will grow (new Decision values, new RuleMatch fields, new
// top-level sections) as policy/proxy/remote subcommands land. Two rules
// keep older config files loadable by newer builds:
//
//  1. New fields and new Decision values are additive only. A config file
//     that predates a new field must resolve to that field's safe default,
//     never to a more permissive behavior than the file's author intended.
//  2. An unrecognized Decision string is rejected (validate fails closed),
//     intentionally — see TestLoad_InvalidDecision. A typo or a decision
//     name from a newer schema version must never silently fall through to
//     an implicit allow.
package config

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/BurntSushi/toml"
)

// Decision represents an allowed policy decision for a tool call.
type Decision string

const (
	Allow    Decision = "allow"
	Ask      Decision = "ask"
	Deny     Decision = "deny"
	Redact   Decision = "redact"
	ReadOnly Decision = "readonly"
	Sandbox  Decision = "sandbox"
)

// Defaults maps capability names to default decisions. Keys correspond to
// risk classes defined in IDEA.md (e.g. "shell", "read_secret", "write_file",
// "network").
type Defaults map[string]Decision

// RuleMatch defines the matching criteria for a policy rule. At least one
// field must be set. Multiple fields are ANDed together.
type RuleMatch struct {
	Server          string   `toml:"server,omitempty" json:"server,omitempty"`
	Tool            string   `toml:"tool,omitempty" json:"tool,omitempty"`
	Capability      string   `toml:"capability,omitempty" json:"capability,omitempty"`
	CommandContains []string `toml:"command_contains,omitempty" json:"command_contains,omitempty"`
}

// Rule maps a match pattern to a policy decision. Rules are evaluated in
// order; the first matching rule wins.
type Rule struct {
	Match    RuleMatch `toml:"match" json:"match"`
	Decision Decision  `toml:"decision" json:"decision"`
}

// ProxyConfig holds upstream MCP server configuration for proxy mode.
type ProxyConfig struct {
	Upstream string `toml:"upstream,omitempty"`
}

// AuditConfig controls the append-only audit log.
type AuditConfig struct {
	Path       string `toml:"path,omitempty"`
	Encrypt    bool   `toml:"encrypt,omitempty"`
	EncryptAge string `toml:"encrypt_age,omitempty"`
}

// RemoteTarget describes a known remote MCP target.
type RemoteTarget struct {
	Name           string   `toml:"name"`
	Provider       string   `toml:"provider"`
	Host           string   `toml:"host"`
	AllowedServers []string `toml:"allowed_servers,omitempty"`
	TrustLevel     string   `toml:"trust_level,omitempty"`
	Labels         []string `toml:"labels,omitempty"`
}

// SequenceConfig enables the optional stateful repetition detector
// (internal/sequence). It is off by default so symguard stays stateless
// unless explicitly opted in. Threshold is the in-window call count that
// blocks a repeated tool+input signature (0 or absent resolves to 3).
type SequenceConfig struct {
	Enabled   bool `toml:"enabled,omitempty"`
	Threshold int  `toml:"threshold,omitempty"`
}

// SpawnEntry is a single allowlisted stdio MCP server launch. Path is the
// absolute path of the executable; ArgvPrefix optionally constrains the
// leading arguments the server may be launched with. An empty ArgvPrefix
// matches any argv.
type SpawnEntry struct {
	Path       string   `toml:"path"`
	ArgvPrefix []string `toml:"argv_prefix,omitempty"`
}

// SpawnConfig governs how stdio MCP servers are launched. The allowlist is
// deny by default: an empty allowlist permits no launch at all.
type SpawnConfig struct {
	Allowlist []SpawnEntry `toml:"allowlist"`
}

// Config is the top-level TOML configuration structure for symguard.
type Config struct {
	Defaults Defaults       `toml:"defaults"`
	Rules    []Rule         `toml:"rules"`
	Proxy    ProxyConfig    `toml:"proxy"`
	Audit    AuditConfig    `toml:"audit"`
	Remote   []RemoteTarget `toml:"remote"`
	Sequence SequenceConfig `toml:"sequence"`
	Spawn    SpawnConfig    `toml:"spawn"`
}

// DefaultConfig returns a Config with sensible defaults. When no config file
// is present, Load returns this value.
func DefaultConfig() *Config {
	return &Config{
		Defaults: Defaults{
			"shell":       Ask,
			"read_secret": Deny,
			"write_file":  Ask,
			"network":     Ask,
		},
		Rules: nil,
		Proxy: ProxyConfig{},
		Audit: AuditConfig{
			Path: "symguard-audit.log",
		},
		Remote: nil,
		Sequence: SequenceConfig{
			Enabled:   false, // opt-in: stateless by default
			Threshold: 3,
		},
		Spawn: SpawnConfig{},
	}
}

// DataDir returns the XDG data directory for symguard:
//
//	$XDG_DATA_HOME/symguard
//
// with a fallback to ~/.local/share/symguard when XDG_DATA_HOME is unset.
// On a UserHomeDir error it falls back to os.TempDir()/symguard.
func DataDir() string {
	if xdg := os.Getenv("XDG_DATA_HOME"); xdg != "" {
		return filepath.Join(xdg, "symguard")
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return filepath.Join(os.TempDir(), "symguard")
	}
	return filepath.Join(home, ".local", "share", "symguard")
}

// DefaultPath returns the XDG Base Directory path for the config file:
//
//	$XDG_CONFIG_HOME/symguard/config.toml
//
// When XDG_CONFIG_HOME is unset, it falls back to ~/.config/symguard/config.toml.
func DefaultPath() string {
	if xdg := os.Getenv("XDG_CONFIG_HOME"); xdg != "" {
		return filepath.Join(xdg, "symguard", "config.toml")
	}
	home, err := os.UserHomeDir()
	if err != nil {
		// Last resort: relative path.
		return filepath.Join(".config", "symguard", "config.toml")
	}
	return filepath.Join(home, ".config", "symguard", "config.toml")
}

// ConfigPath resolves the config file path. It checks SYMGUARD_CONFIG first,
// then falls back to the XDG default path.
func ConfigPath() string {
	if env := os.Getenv("SYMGUARD_CONFIG"); env != "" {
		return env
	}
	return DefaultPath()
}

// Load reads the TOML configuration from the resolved path. When the file
// does not exist it returns DefaultConfig with no error. When the file exists
// but contains invalid TOML or schema violations, it returns a descriptive
// error.
func Load() (*Config, error) {
	return LoadFrom(ConfigPath())
}

// LoadFrom reads the TOML configuration from the given path. When the file
// does not exist it returns DefaultConfig with no error. When the file exists
// but contains invalid TOML, it returns a descriptive error.
func LoadFrom(path string) (*Config, error) {
	cfg := DefaultConfig()

	_, err := os.Stat(path)
	if os.IsNotExist(err) {
		return cfg, nil
	}
	if err != nil {
		return nil, fmt.Errorf("config: stat %s: %w", path, err)
	}

	meta, err := toml.DecodeFile(path, cfg)
	if err != nil {
		return nil, fmt.Errorf("config: parse %s: %w", path, err)
	}

	// Warn about unknown keys so users catch typos early.
	if undecoded := meta.Undecoded(); len(undecoded) > 0 {
		for _, key := range undecoded {
			fmt.Fprintf(os.Stderr, "config: warning: unknown key %q in %s\n", key, path)
		}
	}

	if err := validate(cfg); err != nil {
		return nil, fmt.Errorf("config: validate %s: %w", path, err)
	}

	return cfg, nil
}

// validate checks the decoded config for obvious policy errors.
func validate(cfg *Config) error {
	validDecisions := map[Decision]bool{
		Allow: true, Ask: true, Deny: true, Redact: true, ReadOnly: true, Sandbox: true,
	}

	// An omitted [sequence] threshold decodes to 0; resolve it to the
	// documented default before validation so explicit invalid values
	// (1, negative) stay distinguishable.
	if cfg.Sequence.Enabled && cfg.Sequence.Threshold == 0 {
		cfg.Sequence.Threshold = 3
	}

	for cap, d := range cfg.Defaults {
		if !validDecisions[d] {
			return fmt.Errorf("defaults.%q: invalid decision %q (allowed: allow, ask, deny, redact, readonly, sandbox)", cap, d)
		}
	}

	for i, rule := range cfg.Rules {
		if !validDecisions[rule.Decision] {
			return fmt.Errorf("rules[%d].decision: invalid decision %q", i, rule.Decision)
		}
		if rule.Match.Server == "" && rule.Match.Tool == "" && rule.Match.Capability == "" && len(rule.Match.CommandContains) == 0 {
			return fmt.Errorf("rules[%d]: match must specify at least one criterion (server, tool, capability, command_contains)", i)
		}
	}

	if cfg.Sequence.Enabled && cfg.Sequence.Threshold < 2 {
		return fmt.Errorf("sequence.threshold: must be at least 2 when sequence is enabled (got %d)", cfg.Sequence.Threshold)
	}
	for i, entry := range cfg.Spawn.Allowlist {
		if entry.Path == "" {
			return fmt.Errorf("spawn.allowlist[%d]: path is required", i)
		}
		if !filepath.IsAbs(entry.Path) {
			return fmt.Errorf("spawn.allowlist[%d]: path %q must be absolute", i, entry.Path)
		}
	}

	return nil
}
