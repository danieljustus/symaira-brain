package harness

import (
	"errors"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-corekit/exitcodes"
)

func TestLookup_KnownHarnesses(t *testing.T) {
	for _, name := range []string{"claude", "claude-desktop", "cursor", "opencode", "codex", "gemini"} {
		t.Run(name, func(t *testing.T) {
			h, err := Lookup(name)
			if err != nil {
				t.Fatalf("Lookup(%q): %v", name, err)
			}
			if string(h.Name) != name {
				t.Errorf("h.Name = %q, want %q", h.Name, name)
			}
		})
	}
}

func TestLookup_Unknown(t *testing.T) {
	_, err := Lookup("does-not-exist")
	if err == nil {
		t.Fatal("Lookup() = nil error, want an error")
	}
	var cliErr *exitcodes.CLIError
	if !errors.As(err, &cliErr) {
		t.Fatalf("error is not *exitcodes.CLIError: %v (%T)", err, err)
	}
	if cliErr.Code != exitcodes.ExitNoInput {
		t.Errorf("Code = %d, want %d (ExitNoInput)", cliErr.Code, exitcodes.ExitNoInput)
	}
}

func TestAll_SixHarnesses(t *testing.T) {
	if len(All) != 6 {
		t.Fatalf("len(All) = %d, want 6", len(All))
	}
	for _, h := range All {
		if h.ConfigPath == nil {
			t.Errorf("%s: ConfigPath is nil", h.Name)
		}
		if h.ServersKey == "" {
			t.Errorf("%s: ServersKey is empty", h.Name)
		}
		if h.Format != FormatJSON && h.Format != FormatTOML {
			t.Errorf("%s: Format = %q, want json or toml", h.Name, h.Format)
		}
	}
}

func TestOnlyClaudeSupportsProject(t *testing.T) {
	for _, h := range All {
		want := h.Name == Claude
		if h.SupportsProject != want {
			t.Errorf("%s: SupportsProject = %v, want %v", h.Name, h.SupportsProject, want)
		}
		if want && h.ProjectConfigPath == nil {
			t.Errorf("%s: SupportsProject is true but ProjectConfigPath is nil", h.Name)
		}
	}
}

func TestClaude_ProjectConfigPath(t *testing.T) {
	h, _ := Lookup("claude")
	got := h.ProjectConfigPath("/some/project")
	want := filepath.Join("/some/project", ".mcp.json")
	if got != want {
		t.Errorf("ProjectConfigPath = %q, want %q", got, want)
	}
}

func TestNames_SortedAndComplete(t *testing.T) {
	names := Names()
	if len(names) != len(All) {
		t.Fatalf("len(Names()) = %d, want %d", len(names), len(All))
	}
	for i := 1; i < len(names); i++ {
		if names[i-1] > names[i] {
			t.Errorf("Names() not sorted: %v", names)
			break
		}
	}
}

func TestConfigPath_UsesHome(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", "")

	cases := []struct {
		harness string
		want    string
	}{
		{"claude", filepath.Join(home, ".claude.json")},
		{"cursor", filepath.Join(home, ".cursor", "mcp.json")},
		{"codex", filepath.Join(home, ".codex", "config.toml")},
		{"gemini", filepath.Join(home, ".gemini", "settings.json")},
	}
	for _, tc := range cases {
		t.Run(tc.harness, func(t *testing.T) {
			h, err := Lookup(tc.harness)
			if err != nil {
				t.Fatalf("Lookup: %v", err)
			}
			got, err := h.ConfigPath()
			if err != nil {
				t.Fatalf("ConfigPath(): %v", err)
			}
			if got != tc.want {
				t.Errorf("ConfigPath() = %q, want %q", got, tc.want)
			}
		})
	}
}

func TestOpencodeConfigPath_RespectsXDGConfigHome(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)

	t.Run("default", func(t *testing.T) {
		t.Setenv("XDG_CONFIG_HOME", "")
		h, _ := Lookup("opencode")
		got, err := h.ConfigPath()
		if err != nil {
			t.Fatalf("ConfigPath(): %v", err)
		}
		want := filepath.Join(home, ".config", "opencode", "config.json")
		if got != want {
			t.Errorf("ConfigPath() = %q, want %q", got, want)
		}
	})

	t.Run("xdg override", func(t *testing.T) {
		xdg := t.TempDir()
		t.Setenv("XDG_CONFIG_HOME", xdg)
		h, _ := Lookup("opencode")
		got, err := h.ConfigPath()
		if err != nil {
			t.Fatalf("ConfigPath(): %v", err)
		}
		want := filepath.Join(xdg, "opencode", "config.json")
		if got != want {
			t.Errorf("ConfigPath() = %q, want %q", got, want)
		}
	})
}

func TestResolveClaudeDesktopConfigPath(t *testing.T) {
	env := func(vars map[string]string) func(string) string {
		return func(k string) string { return vars[k] }
	}

	cases := []struct {
		name string
		goos string
		env  map[string]string
		want string
	}{
		{
			name: "darwin",
			goos: "darwin",
			env:  map[string]string{"HOME": "/Users/ada"},
			want: "/Users/ada/Library/Application Support/Claude/claude_desktop_config.json",
		},
		{
			name: "linux default",
			goos: "linux",
			env:  map[string]string{"HOME": "/home/ada"},
			want: "/home/ada/.config/Claude/claude_desktop_config.json",
		},
		{
			name: "linux xdg override",
			goos: "linux",
			env:  map[string]string{"HOME": "/home/ada", "XDG_CONFIG_HOME": "/home/ada/.xdgconf"},
			want: "/home/ada/.xdgconf/Claude/claude_desktop_config.json",
		},
		{
			name: "windows appdata",
			goos: "windows",
			env:  map[string]string{"APPDATA": `C:\Users\ada\AppData\Roaming`},
			want: filepath.Join(`C:\Users\ada\AppData\Roaming`, "Claude", "claude_desktop_config.json"),
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got, err := resolveClaudeDesktopConfigPath(tc.goos, env(tc.env))
			if err != nil {
				t.Fatalf("resolveClaudeDesktopConfigPath: %v", err)
			}
			if got != tc.want {
				t.Errorf("path = %q, want %q", got, tc.want)
			}
		})
	}
}

func TestLookup_ErrorMessageListsKnownHarnesses(t *testing.T) {
	_, err := Lookup("bogus")
	if err == nil {
		t.Fatal("Lookup() = nil error")
	}
	msg := exitcodes.FormatCLIError(err)
	for _, name := range []string{"claude", "cursor", "codex", "gemini"} {
		if !strings.Contains(msg, name) {
			t.Errorf("error message %q does not mention harness %q", msg, name)
		}
	}
}
