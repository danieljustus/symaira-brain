// Package config loads symbrain's global configuration from
// ~/.config/symbrain/config.toml via corekit/configkit, with SYMBRAIN_*
// environment variable overrides and sensible defaults when the file is
// missing.
package config

import (
	"github.com/danieljustus/symaira-corekit/configkit"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// AppName is the configkit app name: it selects the env var prefix
// (SYMBRAIN_*) and the config file path (~/.config/symbrain/config.toml).
const AppName = "symbrain"

// Config is symbrain's resolved global configuration, shared across
// profiles.
type Config struct {
	// DefaultProfile is the profile name used when a command needs one but
	// none was given explicitly (e.g. a future `symbrain mcp` without
	// --profile).
	DefaultProfile string

	Audit       AuditConfig
	Gateway     GatewayConfig
	UpdateCheck UpdateCheckConfig
	Servers     ServersConfig
	Patterns    PatternsConfig
	Modules     ModulesConfig
}

// ModulesConfig selects which optional capability modules are enabled.
// Every module defaults to disabled: choosing one is an explicit, visible
// action (contract PB-2026-09-09 §7 — "no tool/permission exposure from
// installation alone"), never a side effect of installing or updating
// symbrain. Enabling a module here only affects whether `symbrain setup`/
// `symbrain doctor --fix` install its managed binary; it does not by
// itself add anything to a profile's tool exposure.
type ModulesConfig struct {
	// Browse enables symbrowse as a managed, optional web module.
	Browse  bool
	Operate bool
	Scope   bool
}

// EnabledCores maps this config to the managed-core names (as used in
// internal/managed's manifest) that are currently enabled, for use with
// managed.Manifest.ActiveCores. The mapping from a friendly config key
// (e.g. "modules.browse") to a manifest core name (e.g. "symbrowse") is
// deliberately kept here rather than in internal/managed, which has no
// reason to know about config's field names.
func (m ModulesConfig) EnabledCores() map[string]bool {
	enabled := make(map[string]bool)
	if m.Browse {
		enabled["symbrowse"] = true
	}
	return enabled
}

// AuditConfig controls the JSONL audit log written by the gateway.
type AuditConfig struct {
	// Enabled turns the audit log on or off entirely.
	Enabled bool
	// Verbose additionally logs non-vault argument values (never vault
	// arguments or results, regardless of this setting).
	Verbose bool
}

// GatewayConfig controls gateway-specific forwarding behavior.
type GatewayConfig struct {
	// IdentityInjection sets the mapped backend identity parameter to the
	// active profile name when the caller did not supply its own value.
	// A caller-supplied value always wins and is forwarded unchanged.
	IdentityInjection bool
}

// UpdateCheckConfig controls the optional GitHub release update check.
type UpdateCheckConfig struct {
	Enabled bool
}

// PatternsConfig controls episode recording and pattern promotion (see
// internal/patterns). Recording is metadata-only — server and tool names,
// never arguments or values — and nothing is exposed until a sequence
// recurs across PromotionThreshold sessions.
type PatternsConfig struct {
	// Enabled turns session episode recording on or off entirely.
	Enabled bool
	// PromotionThreshold is the number of distinct sessions a sequence
	// must recur in before it is promoted to an exposable pattern.
	PromotionThreshold int
}

// ServersConfig optionally overrides the binary path for external state-core
// processes. Memory and skills are embedded in symbrain and have no child
// binary to override.
type ServersConfig struct {
	Vault ServerOverride `json:"vault"`
}

// ServerOverride pins a child server's binary path, bypassing PATH lookup.
// An empty BinaryPath means "resolve via exec.LookPath as usual".
type ServerOverride struct {
	BinaryPath string `json:"binary_path"`
}

// fileConfig mirrors Config for TOML/env decoding via configkit. configkit
// only overwrites a plain (non-pointer) field when the decoded value is
// non-zero, so a bool field defaulting to true could never be set back to
// false from the config file — an explicit `enabled = false` and an absent
// key would be indistinguishable. configkit's pointer-field handling does
// not have this gap (it applies whenever the key is present, regardless of
// value), so every true-by-default bool goes through *bool here and is
// resolved to its plain Config counterpart in resolve(). Verbose keeps a
// plain bool: its default is false, so "non-zero only" never loses data.
type fileConfig struct {
	DefaultProfile string                `json:"default_profile"`
	Audit          fileAuditConfig       `json:"audit"`
	Gateway        fileGatewayConfig     `json:"gateway"`
	UpdateCheck    fileUpdateCheckConfig `json:"updatecheck"`
	Servers        ServersConfig         `json:"servers"`
	Patterns       filePatternsConfig    `json:"patterns"`
	Modules        fileModulesConfig     `json:"modules"`
}

type fileAuditConfig struct {
	Enabled *bool `json:"enabled"`
	Verbose bool  `json:"verbose"`
}

type fileGatewayConfig struct {
	IdentityInjection *bool `json:"identity_injection"`
}

type fileUpdateCheckConfig struct {
	Enabled *bool `json:"enabled"`
}

type filePatternsConfig struct {
	Enabled            *bool `json:"enabled"`
	PromotionThreshold int   `json:"promotion_threshold"`
}

// fileModulesConfig mirrors ModulesConfig. Every field defaults to false,
// so — unlike the true-by-default configs above — a plain bool loses no
// information here: absent and explicit-false are indistinguishable, but
// both correctly resolve to "disabled" (see ServersConfig/Verbose for the
// same reasoning already established in this file).
type fileModulesConfig struct {
	Browse  bool `json:"browse"`
	Operate bool `json:"operate"`
	Scope   bool `json:"scope"`
}

func fileDefaults() *fileConfig {
	enabled := true
	identityInjection := true
	return &fileConfig{
		Audit:       fileAuditConfig{Enabled: &enabled, Verbose: false},
		Gateway:     fileGatewayConfig{IdentityInjection: &identityInjection},
		UpdateCheck: fileUpdateCheckConfig{Enabled: &enabled},
		Patterns:    filePatternsConfig{Enabled: &enabled, PromotionThreshold: defaultPromotionThreshold},
	}
}

// defaultPromotionThreshold is the number of distinct sessions a tool
// sequence must recur in before it becomes an exposable pattern. It is
// configurable via [patterns] promotion_threshold.
const defaultPromotionThreshold = 3

// Defaults returns the configuration used for any value not set by the
// config file or an environment override.
func Defaults() *Config {
	return resolve(fileDefaults())
}

func resolve(fc *fileConfig) *Config {
	threshold := fc.Patterns.PromotionThreshold
	if threshold <= 0 {
		threshold = defaultPromotionThreshold
	}
	return &Config{
		DefaultProfile: fc.DefaultProfile,
		Audit: AuditConfig{
			Enabled: derefBool(fc.Audit.Enabled, true),
			Verbose: fc.Audit.Verbose,
		},
		Gateway: GatewayConfig{
			IdentityInjection: derefBool(fc.Gateway.IdentityInjection, true),
		},
		UpdateCheck: UpdateCheckConfig{
			Enabled: derefBool(fc.UpdateCheck.Enabled, true),
		},
		Servers: fc.Servers,
		Patterns: PatternsConfig{
			Enabled:            derefBool(fc.Patterns.Enabled, true),
			PromotionThreshold: threshold,
		},
		Modules: ModulesConfig{
			Browse:  fc.Modules.Browse,
			Operate: fc.Modules.Operate,
			Scope:   fc.Modules.Scope,
		},
	}
}

func derefBool(p *bool, fallback bool) bool {
	if p == nil {
		return fallback
	}
	return *p
}

// Load reads the global config file (if present), applies SYMBRAIN_*
// environment overrides, and fills in Defaults() for everything else.
//
// A missing config file is not an error. A config file that fails to parse
// is returned as an *exitcodes.CLIError with exitcodes.ExitNoInput, so
// callers can propagate the right process exit code via
// exitcodes.ExitCodeFromError.
func Load() (*Config, error) {
	loader := configkit.NewLoader(configkit.Options{AppName: AppName}, fileDefaults)

	fc, err := loader.Load()
	if err != nil {
		return nil, exitcodes.Wrap(err, exitcodes.ExitNoInput, exitcodes.KindConfig,
			"config: failed to load "+configkit.DefaultPath(AppName))
	}
	return resolve(fc), nil
}
