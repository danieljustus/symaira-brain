package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-corekit/exitcodes"
)

func withConfigHome(t *testing.T) string {
	t.Helper()
	home := t.TempDir()
	t.Setenv("HOME", home)
	return home
}

func writeConfigTOML(t *testing.T, home, contents string) string {
	t.Helper()
	dir := filepath.Join(home, ".config", "symbrain")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "config.toml")
	if err := os.WriteFile(path, []byte(contents), 0o600); err != nil {
		t.Fatal(err)
	}
	return path
}

func TestConfigPath(t *testing.T) {
	home := withConfigHome(t)
	var stdout, stderr bytes.Buffer

	code := cmdConfig([]string{"path"}, &stdout, &stderr)

	if code != exitcodes.ExitOK {
		t.Fatalf("code = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}
	want := filepath.Join(home, ".config", "symbrain", "config.toml")
	if got := strings.TrimSpace(stdout.String()); got != want {
		t.Fatalf("path = %q, want %q", got, want)
	}
}

func TestConfigGet_NoKeyPrintsFile(t *testing.T) {
	home := withConfigHome(t)
	writeConfigTOML(t, home, "default_profile = \"personal\"\n")
	var stdout, stderr bytes.Buffer

	code := cmdConfig([]string{"get"}, &stdout, &stderr)

	if code != exitcodes.ExitOK {
		t.Fatalf("code = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}
	if !strings.Contains(stdout.String(), "default_profile") {
		t.Fatalf("stdout = %q, want it to contain the config file", stdout.String())
	}
}

func TestConfigGet_NoFile(t *testing.T) {
	withConfigHome(t)
	var stdout, stderr bytes.Buffer

	code := cmdConfig([]string{"get"}, &stdout, &stderr)

	if code != exitcodes.ExitOK {
		t.Fatalf("code = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}
	if !strings.Contains(stdout.String(), "no config file") {
		t.Fatalf("stdout = %q, want the no-config-file note", stdout.String())
	}
}

func TestConfigGet_DottedKey(t *testing.T) {
	home := withConfigHome(t)
	writeConfigTOML(t, home, "[servers.vault]\nbinary_path = \"/opt/bin/symvault\"\n")
	var stdout, stderr bytes.Buffer

	code := cmdConfig([]string{"get", "servers.vault.binary_path"}, &stdout, &stderr)

	if code != exitcodes.ExitOK {
		t.Fatalf("code = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}
	if got := strings.TrimSpace(stdout.String()); got != "/opt/bin/symvault" {
		t.Fatalf("value = %q, want %q", got, "/opt/bin/symvault")
	}
}

func TestConfigGet_MissingKey(t *testing.T) {
	home := withConfigHome(t)
	writeConfigTOML(t, home, "default_profile = \"personal\"\n")
	var stdout, stderr bytes.Buffer

	code := cmdConfig([]string{"get", "audit.enabled"}, &stdout, &stderr)

	if code != exitcodes.ExitNoInput {
		t.Fatalf("code = %d, want %d", code, exitcodes.ExitNoInput)
	}
	if !strings.Contains(stderr.String(), "not set") {
		t.Fatalf("stderr = %q, want a not-set message", stderr.String())
	}
}

func TestConfigSet_PreservesOtherKeysAndTypes(t *testing.T) {
	home := withConfigHome(t)
	writeConfigTOML(t, home, "default_profile = \"personal\"\n\n[audit]\nenabled = true\n")
	var stdout, stderr bytes.Buffer

	code := cmdConfig([]string{"set", "audit.verbose", "true"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("set code = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	code = cmdConfig([]string{"set", "patterns.promotion_threshold", "5"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("set code = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	code = cmdConfig([]string{"set", "servers.vault.binary_path", "/tmp/symvault"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("set code = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	raw, err := os.ReadFile(filepath.Join(home, ".config", "symbrain", "config.toml"))
	if err != nil {
		t.Fatal(err)
	}
	content := string(raw)
	for _, want := range []string{
		`default_profile = "personal"`,
		`verbose = true`,
		`promotion_threshold = 5`,
		`binary_path = "/tmp/symvault"`,
	} {
		if !strings.Contains(content, want) {
			t.Errorf("config file missing %q:\n%s", want, content)
		}
	}
}

func TestConfigSet_CreatesFile(t *testing.T) {
	home := withConfigHome(t)
	var stdout, stderr bytes.Buffer

	code := cmdConfig([]string{"set", "default_profile", "restricted"}, &stdout, &stderr)

	if code != exitcodes.ExitOK {
		t.Fatalf("code = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}
	raw, err := os.ReadFile(filepath.Join(home, ".config", "symbrain", "config.toml"))
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(raw), `default_profile = "restricted"`) {
		t.Fatalf("config file = %q, want default_profile set", string(raw))
	}
}

func TestConfigSet_TypeConflict(t *testing.T) {
	home := withConfigHome(t)
	writeConfigTOML(t, home, "default_profile = \"personal\"\n")
	var stdout, stderr bytes.Buffer

	code := cmdConfig([]string{"set", "default_profile.x", "y"}, &stdout, &stderr)

	if code != exitcodes.ExitNoInput {
		t.Fatalf("code = %d, want %d", code, exitcodes.ExitNoInput)
	}
	if !strings.Contains(stderr.String(), "not a table") {
		t.Fatalf("stderr = %q, want a not-a-table message", stderr.String())
	}
}

func TestConfig_UnknownSubcommand(t *testing.T) {
	var stdout, stderr bytes.Buffer
	code := cmdConfig([]string{"frobnicate"}, &stdout, &stderr)
	if code != exitcodes.ExitNoInput {
		t.Fatalf("code = %d, want %d", code, exitcodes.ExitNoInput)
	}
	if !strings.Contains(stderr.String(), "unknown subcommand") {
		t.Fatalf("stderr = %q, want unknown-subcommand message", stderr.String())
	}
}

func TestConfig_NoSubcommandPrintsUsage(t *testing.T) {
	var stdout, stderr bytes.Buffer
	code := cmdConfig(nil, &stdout, &stderr)
	if code != exitcodes.ExitNoInput {
		t.Fatalf("code = %d, want %d", code, exitcodes.ExitNoInput)
	}
	if !strings.Contains(stderr.String(), "Usage:") {
		t.Fatalf("stderr = %q, want usage text", stderr.String())
	}
}

func TestInferValue_Types(t *testing.T) {
	cases := []struct {
		in   string
		want string
	}{
		{"true", "bool"},
		{"FALSE", "bool"},
		{"42", "int64"},
		{"-7", "int64"},
		{"symvault", "string"},
		{"4.5", "string"},
	}
	for _, tc := range cases {
		got := inferValue(tc.in)
		if typ := typeName(got); typ != tc.want {
			t.Errorf("inferValue(%q) = %T (%v), want %s", tc.in, got, got, tc.want)
		}
	}
}

func typeName(v any) string {
	switch v.(type) {
	case bool:
		return "bool"
	case int64:
		return "int64"
	default:
		return "string"
	}
}
