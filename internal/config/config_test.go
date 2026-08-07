package config

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// withHome points $HOME at a fresh temp directory for the duration of the
// test, so global config lookups never touch the real user config.
func withHome(t *testing.T) string {
	t.Helper()
	home := t.TempDir()
	t.Setenv("HOME", home)
	return home
}

func writeConfig(t *testing.T, home, contents string) {
	t.Helper()
	dir := filepath.Join(home, ".config", AppName)
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}
	path := filepath.Join(dir, "config.toml")
	if err := os.WriteFile(path, []byte(contents), 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}
}

func TestLoad_MissingFileYieldsDefaults(t *testing.T) {
	withHome(t)

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v, want nil", err)
	}

	want := Defaults()
	if cfg.DefaultProfile != want.DefaultProfile ||
		cfg.Audit != want.Audit ||
		cfg.Gateway != want.Gateway ||
		cfg.UpdateCheck != want.UpdateCheck ||
		cfg.Servers != want.Servers {
		t.Fatalf("Load() = %+v, want defaults %+v", cfg, want)
	}
}

func TestLoad_FileOverridesDefaults(t *testing.T) {
	home := withHome(t)
	writeConfig(t, home, `
default_profile = "personal"

[audit]
enabled = false
verbose = true

[gateway]
identity_injection = false

[servers.vault]
binary_path = "/opt/symvault/symvault"
`)

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v, want nil", err)
	}

	if cfg.DefaultProfile != "personal" {
		t.Errorf("DefaultProfile = %q, want %q", cfg.DefaultProfile, "personal")
	}
	if cfg.Audit.Enabled != false || cfg.Audit.Verbose != true {
		t.Errorf("Audit = %+v, want {Enabled:false Verbose:true}", cfg.Audit)
	}
	if cfg.Gateway.IdentityInjection {
		t.Error("Gateway.IdentityInjection = true, want false")
	}
	if cfg.Servers.Vault.BinaryPath != "/opt/symvault/symvault" {
		t.Errorf("Servers.Vault.BinaryPath = %q, want %q", cfg.Servers.Vault.BinaryPath, "/opt/symvault/symvault")
	}
	// Untouched keys still fall back to defaults.
	if cfg.UpdateCheck.Enabled != true {
		t.Errorf("UpdateCheck.Enabled = %v, want true (default)", cfg.UpdateCheck.Enabled)
	}
	if cfg.Recipes.Enabled != true || cfg.Recipes.PromotionThreshold != 3 {
		t.Errorf("Recipes = %+v, want {Enabled:true PromotionThreshold:3} (defaults)", cfg.Recipes)
	}
}

func TestLoad_RecipesOverrides(t *testing.T) {
	home := withHome(t)
	writeConfig(t, home, `
[recipes]
enabled = false
promotion_threshold = 5
`)

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v, want nil", err)
	}
	if cfg.Recipes.Enabled {
		t.Error("Recipes.Enabled = true, want false")
	}
	if cfg.Recipes.PromotionThreshold != 5 {
		t.Errorf("Recipes.PromotionThreshold = %d, want 5", cfg.Recipes.PromotionThreshold)
	}
}

func TestLoad_RecipesThresholdSanitized(t *testing.T) {
	home := withHome(t)
	writeConfig(t, home, `
[recipes]
promotion_threshold = 0
`)

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v, want nil", err)
	}
	if cfg.Recipes.PromotionThreshold != 3 {
		t.Errorf("Recipes.PromotionThreshold = %d, want default 3 for a non-positive value", cfg.Recipes.PromotionThreshold)
	}
}

func TestLoad_EnvOverridesFileAndDefaults(t *testing.T) {
	home := withHome(t)
	writeConfig(t, home, `default_profile = "personal"`)

	tests := []struct {
		name   string
		envs   map[string]string
		verify func(t *testing.T, cfg *Config)
	}{
		{
			name: "env overrides file value",
			envs: map[string]string{"SYMBRAIN_DEFAULT_PROFILE": "restricted"},
			verify: func(t *testing.T, cfg *Config) {
				if cfg.DefaultProfile != "restricted" {
					t.Errorf("DefaultProfile = %q, want %q", cfg.DefaultProfile, "restricted")
				}
			},
		},
		{
			name: "env overrides default bool",
			envs: map[string]string{"SYMBRAIN_AUDIT_ENABLED": "false"},
			verify: func(t *testing.T, cfg *Config) {
				if cfg.Audit.Enabled {
					t.Errorf("Audit.Enabled = true, want false")
				}
			},
		},
		{
			name: "env overrides nested server binary path",
			envs: map[string]string{"SYMBRAIN_SERVERS_SKILLS_BINARY_PATH": "/custom/symskills"},
			verify: func(t *testing.T, cfg *Config) {
				if cfg.Servers.Skills.BinaryPath != "/custom/symskills" {
					t.Errorf("Servers.Skills.BinaryPath = %q, want %q", cfg.Servers.Skills.BinaryPath, "/custom/symskills")
				}
			},
		},
		{
			name: "env disables identity injection",
			envs: map[string]string{"SYMBRAIN_GATEWAY_IDENTITY_INJECTION": "false"},
			verify: func(t *testing.T, cfg *Config) {
				if cfg.Gateway.IdentityInjection {
					t.Error("Gateway.IdentityInjection = true, want false")
				}
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			for k, v := range tt.envs {
				t.Setenv(k, v)
			}

			cfg, err := Load()
			if err != nil {
				t.Fatalf("Load() error = %v, want nil", err)
			}
			tt.verify(t, cfg)
		})
	}
}

func TestLoad_InvalidTOMLReturnsExitNoInput(t *testing.T) {
	home := withHome(t)
	writeConfig(t, home, `default_profile = "unterminated`)

	_, err := Load()
	if err == nil {
		t.Fatal("Load() error = nil, want a parse error")
	}

	if got := exitcodes.ExitCodeFromError(err); got != exitcodes.ExitNoInput {
		t.Errorf("ExitCodeFromError(err) = %d, want %d", got, exitcodes.ExitNoInput)
	}
	if exitcodes.FormatCLIError(err) == "" {
		t.Error("FormatCLIError(err) is empty, want a clear message")
	}
}
