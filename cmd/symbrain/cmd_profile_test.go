package main

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/profile"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// writeProfileFile writes a minimal valid profile TOML for name under
// home/.config/symbrain/profiles/<name>.toml.
func writeProfileFile(t *testing.T, home, name, contents string) {
	t.Helper()
	dir := filepath.Join(home, ".config", "symbrain", "profiles")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}
	path := filepath.Join(dir, name+".toml")
	if err := os.WriteFile(path, []byte(contents), 0o644); err != nil {
		t.Fatalf("WriteFile(%s): %v", name, err)
	}
}

func TestCmdProfile_NoSubcommandPrintsUsage(t *testing.T) {
	var stdout, stderr bytes.Buffer
	code := cmdProfile(nil, &stdout, &stderr)
	if code != exitcodes.ExitNoInput {
		t.Fatalf("cmdProfile(nil) = %d, want %d", code, exitcodes.ExitNoInput)
	}
	if !strings.Contains(stderr.String(), "Usage:") {
		t.Errorf("stderr = %q, want usage text", stderr.String())
	}
}

func TestCmdProfile_UnknownSubcommand(t *testing.T) {
	var stdout, stderr bytes.Buffer
	code := cmdProfile([]string{"bogus"}, &stdout, &stderr)
	if code != exitcodes.ExitNoInput {
		t.Fatalf("cmdProfile(bogus) = %d, want %d", code, exitcodes.ExitNoInput)
	}
	if !strings.Contains(stderr.String(), "unknown subcommand") {
		t.Errorf("stderr = %q, want it to mention unknown subcommand", stderr.String())
	}
}

// ---- list ----

func TestCmdProfileList_EmptyDir(t *testing.T) {
	sandboxHome(t)

	var stdout, stderr bytes.Buffer
	code := cmdProfile([]string{"list"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("profile list = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}
	if !strings.Contains(stdout.String(), "no profiles found") {
		t.Errorf("stdout = %q, want it to say no profiles found", stdout.String())
	}
}

func TestCmdProfileList_JSONIsSnakeCaseAndReflectsServers(t *testing.T) {
	home := sandboxHome(t)
	writeProfileFile(t, home, "personal", `
[profile]
name = "personal"
description = "Full access"

[servers.vault]
enabled = true
mode    = "full"

[servers.memory]
enabled = true
mode    = "read_write"

[servers.skills]
enabled = true
`)

	var stdout, stderr bytes.Buffer
	code := cmdProfile([]string{"list", "--json"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("profile list --json = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	var entries []map[string]any
	if err := json.Unmarshal(stdout.Bytes(), &entries); err != nil {
		t.Fatalf("output is not valid JSON: %v (output: %q)", err, stdout.String())
	}
	if len(entries) != 1 {
		t.Fatalf("len(entries) = %d, want 1", len(entries))
	}
	e := entries[0]
	if e["name"] != "personal" {
		t.Errorf("name = %v, want personal", e["name"])
	}
	if e["description"] != "Full access" {
		t.Errorf("description = %v, want %q", e["description"], "Full access")
	}
	servers, ok := e["servers"].([]any)
	if !ok || len(servers) != 3 {
		t.Fatalf("servers = %v, want 3 entries", e["servers"])
	}
}

func TestCmdProfileList_BrokenProfileReportsErrorWithoutFailing(t *testing.T) {
	home := sandboxHome(t)
	writeProfileFile(t, home, "broken", `[profile]
name = "wrong-name"`)

	var stdout, stderr bytes.Buffer
	code := cmdProfile([]string{"list", "--json"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("profile list --json = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	var entries []map[string]any
	if err := json.Unmarshal(stdout.Bytes(), &entries); err != nil {
		t.Fatalf("output is not valid JSON: %v", err)
	}
	if len(entries) != 1 || entries[0]["name"] != "broken" || entries[0]["error"] == "" || entries[0]["error"] == nil {
		t.Errorf("entries = %v, want one entry named broken with a non-empty error", entries)
	}
}

// ---- show ----

func TestCmdProfileShow_MissingProfileExitsNoInput(t *testing.T) {
	sandboxHome(t)

	var stdout, stderr bytes.Buffer
	code := cmdProfile([]string{"show", "ghost"}, &stdout, &stderr)
	if code != exitcodes.ExitNoInput {
		t.Fatalf("profile show ghost = %d, want %d (stderr: %s)", code, exitcodes.ExitNoInput, stderr.String())
	}
}

func TestCmdProfileShow_JSONIncludesEffectivePolicyForVaultAndMemory(t *testing.T) {
	home := sandboxHome(t)
	writeProfileFile(t, home, "restricted", `
[profile]
name = "restricted"
description = "Least-privilege"

[servers.vault]
enabled = true
mode    = "request_only"

[servers.memory]
enabled = true
mode    = "read_only"

[servers.skills]
enabled = true
`)

	var stdout, stderr bytes.Buffer
	code := cmdProfile([]string{"show", "restricted", "--json"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("profile show restricted --json = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	var report struct {
		Name    string `json:"name"`
		Servers []struct {
			Server          string `json:"server"`
			Mode            string `json:"mode"`
			EffectivePolicy *struct {
				Exposed []string `json:"exposed"`
			} `json:"effective_policy"`
			Note string `json:"note"`
		} `json:"servers"`
	}
	if err := json.Unmarshal(stdout.Bytes(), &report); err != nil {
		t.Fatalf("output is not valid JSON: %v (output: %q)", err, stdout.String())
	}
	if report.Name != "restricted" {
		t.Errorf("name = %q, want restricted", report.Name)
	}

	var vault, skills *struct {
		Server          string `json:"server"`
		Mode            string `json:"mode"`
		EffectivePolicy *struct {
			Exposed []string `json:"exposed"`
		} `json:"effective_policy"`
		Note string `json:"note"`
	}
	for i := range report.Servers {
		switch report.Servers[i].Server {
		case "vault":
			vault = &report.Servers[i]
		case "skills":
			skills = &report.Servers[i]
		}
	}
	if vault == nil || vault.EffectivePolicy == nil {
		t.Fatalf("vault server report = %+v, want a non-nil effective_policy", vault)
	}
	found := false
	for _, tool := range vault.EffectivePolicy.Exposed {
		if tool == "request_credential" {
			found = true
		}
	}
	if !found {
		t.Errorf("vault effective_policy.exposed = %v, want it to contain request_credential", vault.EffectivePolicy.Exposed)
	}
	if skills == nil || skills.Note == "" {
		t.Errorf("skills server report = %+v, want a non-empty note (no mode preset)", skills)
	}
}

func TestCmdProfileShow_HumanOutputIncludesWarnings(t *testing.T) {
	home := sandboxHome(t)
	writeProfileFile(t, home, "warny", `
[profile]
name   = "warny"
author = "someone"
`)

	var stdout, stderr bytes.Buffer
	code := cmdProfile([]string{"show", "warny"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("profile show warny = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}
	if !strings.Contains(stdout.String(), "warnings:") {
		t.Errorf("stdout = %q, want it to mention warnings", stdout.String())
	}
}

// ---- add ----

func TestCmdProfileAdd_FromRestrictedIsDefault(t *testing.T) {
	home := sandboxHome(t)

	var stdout, stderr bytes.Buffer
	code := cmdProfile([]string{"add", "myprofile"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("profile add myprofile = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	path := filepath.Join(home, ".config", "symbrain", "profiles", "myprofile.toml")
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("expected %s to exist: %v", path, err)
	}
	if !strings.Contains(string(data), `name        = "myprofile"`) &&
		!strings.Contains(string(data), `name = "myprofile"`) {
		t.Errorf("written profile does not contain a rewritten name field:\n%s", data)
	}
	if !strings.Contains(string(data), "request_only") {
		t.Errorf("written profile = %q, want it to come from the restricted template (request_only)", data)
	}
}

func TestCmdProfileAdd_FromPersonalFlagAfterName(t *testing.T) {
	home := sandboxHome(t)

	var stdout, stderr bytes.Buffer
	// Flags after the positional name — exercises reorderFlagsFirst.
	code := cmdProfile([]string{"add", "myprofile", "--from", "personal"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("profile add myprofile --from personal = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	path := filepath.Join(home, ".config", "symbrain", "profiles", "myprofile.toml")
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("expected %s to exist: %v", path, err)
	}
	if !strings.Contains(string(data), `"myprofile"`) {
		t.Errorf("written profile does not reference the new name:\n%s", data)
	}
	if strings.Contains(string(data), `"personal"`) {
		t.Errorf("written profile still contains the template placeholder name:\n%s", data)
	}
	if !strings.Contains(string(data), "read_write") {
		t.Errorf("written profile = %q, want it to come from the personal template (read_write)", data)
	}

	// The written file must itself be loadable (name matches filename).
	p, err := profile.Load("myprofile")
	if err != nil {
		t.Fatalf("Load(myprofile) after add: %v", err)
	}
	if p.Name != "myprofile" {
		t.Errorf("loaded profile Name = %q, want myprofile", p.Name)
	}
}

func TestCmdProfileAdd_AlreadyExists(t *testing.T) {
	home := sandboxHome(t)
	writeProfileFile(t, home, "dup", `[profile]
name = "dup"`)

	var stdout, stderr bytes.Buffer
	code := cmdProfile([]string{"add", "dup"}, &stdout, &stderr)
	if code != exitcodes.ExitNoInput {
		t.Fatalf("profile add dup (exists) = %d, want %d (stderr: %s)", code, exitcodes.ExitNoInput, stderr.String())
	}
}

func TestCmdProfileAdd_InvalidNameRejected(t *testing.T) {
	sandboxHome(t)

	var stdout, stderr bytes.Buffer
	code := cmdProfile([]string{"add", "../evil"}, &stdout, &stderr)
	if code != exitcodes.ExitNoInput {
		t.Fatalf("profile add ../evil = %d, want %d (stderr: %s)", code, exitcodes.ExitNoInput, stderr.String())
	}
}

func TestCmdProfileAdd_InvalidFromRejected(t *testing.T) {
	sandboxHome(t)

	var stdout, stderr bytes.Buffer
	code := cmdProfile([]string{"add", "x", "--from", "nope"}, &stdout, &stderr)
	if code != exitcodes.ExitNoInput {
		t.Fatalf("profile add x --from nope = %d, want %d (stderr: %s)", code, exitcodes.ExitNoInput, stderr.String())
	}
}

// ---- remove ----

func TestCmdProfileRemove_ForceDeletesWithoutPrompt(t *testing.T) {
	home := sandboxHome(t)
	writeProfileFile(t, home, "gone", `[profile]
name = "gone"`)

	var stdout, stderr bytes.Buffer
	code := cmdProfile([]string{"remove", "gone", "--force"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("profile remove gone --force = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	path := filepath.Join(home, ".config", "symbrain", "profiles", "gone.toml")
	if _, err := os.Stat(path); !os.IsNotExist(err) {
		t.Errorf("expected %s to be removed, stat err = %v", path, err)
	}
}

func TestCmdProfileRemove_MissingProfile(t *testing.T) {
	sandboxHome(t)

	var stdout, stderr bytes.Buffer
	code := cmdProfile([]string{"remove", "ghost", "--force"}, &stdout, &stderr)
	if code != exitcodes.ExitNoInput {
		t.Fatalf("profile remove ghost --force = %d, want %d (stderr: %s)", code, exitcodes.ExitNoInput, stderr.String())
	}
}

func TestCmdProfileRemove_PromptDeclineKeepsFile(t *testing.T) {
	home := sandboxHome(t)
	writeProfileFile(t, home, "keepme", `[profile]
name = "keepme"`)

	oldReader := confirmReader
	confirmReader = strings.NewReader("n\n")
	defer func() { confirmReader = oldReader }()

	var stdout, stderr bytes.Buffer
	code := cmdProfile([]string{"remove", "keepme"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("profile remove keepme (declined) = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	path := filepath.Join(home, ".config", "symbrain", "profiles", "keepme.toml")
	if _, err := os.Stat(path); err != nil {
		t.Errorf("expected %s to still exist after declining: %v", path, err)
	}
	if !strings.Contains(stdout.String(), "aborted") {
		t.Errorf("stdout = %q, want it to mention aborted", stdout.String())
	}
}

func TestCmdProfileRemove_PromptAcceptDeletesFile(t *testing.T) {
	home := sandboxHome(t)
	writeProfileFile(t, home, "byebye", `[profile]
name = "byebye"`)

	oldReader := confirmReader
	confirmReader = strings.NewReader("y\n")
	defer func() { confirmReader = oldReader }()

	var stdout, stderr bytes.Buffer
	code := cmdProfile([]string{"remove", "byebye"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("profile remove byebye (accepted) = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	path := filepath.Join(home, ".config", "symbrain", "profiles", "byebye.toml")
	if _, err := os.Stat(path); !os.IsNotExist(err) {
		t.Errorf("expected %s to be removed after accepting, stat err = %v", path, err)
	}
}

// ---- reorderFlagsFirst ----

func TestReorderFlagsFirst(t *testing.T) {
	tests := []struct {
		name       string
		args       []string
		valueFlags map[string]bool
		want       []string
	}{
		{"empty", nil, nil, nil},
		{"flag already first", []string{"--json", "name"}, nil, []string{"--json", "name"}},
		{"flag after positional", []string{"name", "--json"}, nil, []string{"--json", "name"}},
		{
			"value flag after positional",
			[]string{"name", "--from", "personal"},
			map[string]bool{"from": true},
			[]string{"--from", "personal", "name"},
		},
		{
			"value flag before positional",
			[]string{"--from", "personal", "name"},
			map[string]bool{"from": true},
			[]string{"--from", "personal", "name"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := reorderFlagsFirst(tt.args, tt.valueFlags)
			if len(got) != len(tt.want) {
				t.Fatalf("reorderFlagsFirst(%v) = %v, want %v", tt.args, got, tt.want)
			}
			for i := range got {
				if got[i] != tt.want[i] {
					t.Fatalf("reorderFlagsFirst(%v) = %v, want %v", tt.args, got, tt.want)
				}
			}
		})
	}
}
