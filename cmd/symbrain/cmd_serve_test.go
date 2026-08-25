package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// withServeHome points $HOME at a fresh temp directory so profile.Load
// never touches the real user config.
func withServeHome(t *testing.T) string {
	t.Helper()
	home := t.TempDir()
	t.Setenv("HOME", home)
	return home
}

func TestResolveServeProfile_ByName(t *testing.T) {
	home := withServeHome(t)
	dir := filepath.Join(home, ".config", "symbrain", "profiles")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}
	path := filepath.Join(dir, "personal.toml")
	contents := "[profile]\nname = \"personal\"\n"
	if err := os.WriteFile(path, []byte(contents), 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	p, err := resolveServeProfile("personal", "")
	if err != nil {
		t.Fatalf("resolveServeProfile() error = %v, want nil", err)
	}
	if p.Name != "personal" {
		t.Errorf("Name = %q, want %q", p.Name, "personal")
	}
}

func TestResolveServeProfile_ByFile(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "room.toml")
	contents := "[profile]\nname = \"room\"\n\n[servers.memory]\nenabled = true\nmode = \"read_only\"\n"
	if err := os.WriteFile(path, []byte(contents), 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	p, err := resolveServeProfile("", path)
	if err != nil {
		t.Fatalf("resolveServeProfile() error = %v, want nil", err)
	}
	if p.Name != "room" {
		t.Errorf("Name = %q, want %q", p.Name, "room")
	}
	if !p.Servers.Memory.Enabled {
		t.Error("Memory should be enabled")
	}
}

func TestResolveServeProfile_Errors(t *testing.T) {
	t.Run("neither source", func(t *testing.T) {
		_, err := resolveServeProfile("", "")
		if err == nil {
			t.Fatal("resolveServeProfile() error = nil, want error")
		}
	})

	t.Run("both sources", func(t *testing.T) {
		_, err := resolveServeProfile("personal", "/tmp/room.toml")
		if err == nil {
			t.Fatal("resolveServeProfile() error = nil, want error")
		}
	})

	t.Run("missing file", func(t *testing.T) {
		_, err := resolveServeProfile("", filepath.Join(t.TempDir(), "nope.toml"))
		if err == nil {
			t.Fatal("resolveServeProfile() error = nil, want error for a missing file")
		}
	})
}

// TestRun_McpAndServeDispatch verifies that `symbrain mcp` routes to the
// stdio gateway (it fails with the profile-required usage error, proving
// dispatch reached the gateway) and that the deprecated `serve` alias still
// works with its notice on stderr only. stdout must stay empty in both
// cases: it is the MCP JSON-RPC transport (Zero Stdio Pollution).
func TestRun_McpAndServeDispatch(t *testing.T) {
	withServeHome(t)

	t.Run("mcp routes to gateway", func(t *testing.T) {
		var stdout, stderr bytes.Buffer
		code := run([]string{"mcp"}, &stdout, &stderr)
		if code != exitcodes.ExitNoInput {
			t.Fatalf("code = %d, want %d (stderr: %s)", code, exitcodes.ExitNoInput, stderr.String())
		}
		if !strings.Contains(stderr.String(), "symbrain mcp:") {
			t.Fatalf("stderr = %q, want gateway error prefix", stderr.String())
		}
		if stdout.Len() != 0 {
			t.Fatalf("stdout = %q, want empty (zero stdio pollution)", stdout.String())
		}
		if strings.Contains(stderr.String(), "deprecated") {
			t.Fatalf("stderr = %q, mcp must not print a deprecation notice", stderr.String())
		}
	})

	t.Run("serve works with stderr-only deprecation", func(t *testing.T) {
		var stdout, stderr bytes.Buffer
		code := run([]string{"serve"}, &stdout, &stderr)
		if code != exitcodes.ExitNoInput {
			t.Fatalf("code = %d, want %d (stderr: %s)", code, exitcodes.ExitNoInput, stderr.String())
		}
		if !strings.Contains(stderr.String(), "deprecated") {
			t.Fatalf("stderr = %q, want deprecation notice", stderr.String())
		}
		if !strings.Contains(stderr.String(), "symbrain mcp:") {
			t.Fatalf("stderr = %q, want gateway error prefix", stderr.String())
		}
		if stdout.Len() != 0 {
			t.Fatalf("stdout = %q, want empty (zero stdio pollution)", stdout.String())
		}
	})
}
