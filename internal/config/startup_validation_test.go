package config

import (
	"strings"
	"testing"

	"github.com/danieljustus/symaira-corekit/exitcodes"
)

func TestLoad_AuditFileValidationPrecedesEnvironmentOverride(t *testing.T) {
	cases := []struct {
		name, contents string
	}{
		{"malformed", "[audit\n"},
		{"invalid_value", "[audit]\nenabled = \"yes\"\n"},
		{"invalid_type", "[audit]\nenabled = []\n"},
	}
	for _, tc := range cases {
		for _, override := range []string{"true", "false", "invalid"} {
			t.Run(tc.name+"/"+override, func(t *testing.T) {
				home := withHome(t)
				t.Setenv("USERPROFILE", home)
				t.Setenv("XDG_CONFIG_HOME", "")
				t.Setenv("SYMBRAIN_AUDIT_ENABLED", override)
				writeConfig(t, home, tc.contents)
				cfg, err := Load()
				if err == nil || cfg != nil {
					t.Fatalf("invalid file was hidden by environment override: cfg=%+v err=%v", cfg, err)
				}
				if got := exitcodes.ExitCodeFromError(err); got != exitcodes.ExitNoInput {
					t.Fatalf("exit = %d, want %d", got, exitcodes.ExitNoInput)
				}
				if strings.Contains(err.Error(), "SYMBRAIN_AUDIT_ENABLED") {
					t.Fatalf("environment error displaced file error: %v", err)
				}
			})
		}
	}
}

func TestLoad_AuditScalarSectionIsIgnoredBeforeEnvironmentOverride(t *testing.T) {
	home := withHome(t)
	t.Setenv("USERPROFILE", home)
	t.Setenv("XDG_CONFIG_HOME", "")
	writeConfig(t, home, "audit = true\n")
	for _, override := range []string{"true", "false"} {
		t.Run(override, func(t *testing.T) {
			t.Setenv("SYMBRAIN_AUDIT_ENABLED", override)
			cfg, err := Load()
			if err != nil || cfg.Audit.Enabled != (override == "true") {
				t.Fatalf("scalar section/override: cfg=%+v err=%v", cfg, err)
			}
		})
	}
}
