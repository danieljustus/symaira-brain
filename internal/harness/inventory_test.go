package harness

import (
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

func TestList_ConfigPresent(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	h := mustLookup(t, "claude")
	path, err := h.ConfigPath()
	if err != nil {
		t.Fatalf("ConfigPath(): %v", err)
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatalf("MkdirAll(): %v", err)
	}
	doc := Empty(h)
	doc.SetServer("zeta", Entry{Command: "zeta"})
	doc.SetServer("alpha", Entry{Command: "alpha"})
	data, err := doc.Marshal()
	if err != nil {
		t.Fatalf("Marshal(): %v", err)
	}
	if err := os.WriteFile(path, data, 0o600); err != nil {
		t.Fatalf("WriteFile(): %v", err)
	}

	item := findInventory(t, List(""), h.Name)
	if !item.Global.Exists || !item.Global.Parsed {
		t.Fatalf("global state = %+v, want existing and parsed", item.Global)
	}
	if got, want := serverNames(item.Global.Servers), []string{"alpha", "zeta"}; !reflect.DeepEqual(got, want) {
		t.Fatalf("servers = %v, want %v", got, want)
	}
}

func TestList_ConfigMissing(t *testing.T) {
	t.Setenv("HOME", t.TempDir())
	h := mustLookup(t, "cursor")
	item := findInventory(t, List(""), h.Name)
	if item.Global.Exists || item.Global.Parsed {
		t.Fatalf("global state = %+v, want missing", item.Global)
	}
	if len(item.Global.Servers) != 0 {
		t.Fatalf("servers = %v, want empty", item.Global.Servers)
	}
}

func TestList_OnlyMCPInstallHarnesses(t *testing.T) {
	t.Setenv("HOME", t.TempDir())
	report := List("")
	for _, item := range report.Harnesses {
		h, err := Lookup(string(item.Name))
		if err != nil {
			t.Fatalf("Lookup(%q): %v", item.Name, err)
		}
		if !h.SupportsMCPInstall {
			t.Errorf("inventory includes capability-only harness %q", item.Name)
		}
	}
	if got, want := len(report.Harnesses), 6; got != want {
		t.Fatalf("inventory harness count = %d, want %d MCP-installable harnesses", got, want)
	}
}

// TestList_ServerDetail verifies schema-2 inventory carries per-server
// transport detail: command, args, transport inference, url, and env-var
// names for both the JSON and TOML backends.
func TestList_ServerDetail(t *testing.T) {
	for _, h := range []Harness{All[0], mustLookup(t, "codex")} {
		t.Run(string(h.Name), func(t *testing.T) {
			home := t.TempDir()
			t.Setenv("HOME", home)
			path, err := h.ConfigPath()
			if err != nil {
				t.Fatalf("ConfigPath(): %v", err)
			}
			if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
				t.Fatalf("MkdirAll(): %v", err)
			}
			doc := Empty(h)
			doc.SetServer("web", Entry{Command: "uvx", Args: []string{"mcp-server", "--port", "9000"}})
			doc.SetServer("remote", Entry{Command: ""})
			// For the JSON backend SetServer writes command/args only, so
			// enrich the fixture with url + env by writing the raw config.
			writeDetailFixture(t, h, path, doc)
			item := findInventory(t, List(""), h.Name)
			servers := serverInfoMap(item.Global.Servers)
			web, ok := servers["web"]
			if !ok {
				t.Fatalf("missing server %q in %v", "web", serverNames(item.Global.Servers))
			}
			if web.Transport != "stdio" || web.Command != "uvx" {
				t.Errorf("web server detail = %+v, want stdio/uvx", web)
			}
			if !reflect.DeepEqual(web.Args, []string{"mcp-server", "--port", "9000"}) {
				t.Errorf("web args = %v, want [mcp-server --port 9000]", web.Args)
			}
			remote, ok := servers["remote"]
			if !ok {
				t.Fatalf("missing server %q", "remote")
			}
			if remote.Transport != "http" || remote.URL != "http://localhost:8080/mcp" {
				t.Errorf("remote server detail = %+v, want http url", remote)
			}
			if !reflect.DeepEqual(remote.EnvNames, []string{"API_KEY", "TOKEN"}) {
				t.Errorf("remote env_names = %v, want [API_KEY TOKEN]", remote.EnvNames)
			}
		})
	}
}

// TestList_NoEnvValuesLeak asserts a config carrying a plaintext key never
// leaks the value through the inventory — only the variable name appears.
func TestList_NoEnvValuesLeak(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	h := mustLookup(t, "claude")
	path, err := h.ConfigPath()
	if err != nil {
		t.Fatalf("ConfigPath(): %v", err)
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatalf("MkdirAll(): %v", err)
	}
	const secret = "sk-live-plaintext-key-that-must-never-leak"
	data := []byte(`{"mcpServers":{"vaultish":{"command":"symvault","env":{"ANTHROPIC_API_KEY":"` + secret + `"}}}}`)
	if err := os.WriteFile(path, data, 0o600); err != nil {
		t.Fatalf("WriteFile(): %v", err)
	}

	report := List("")
	raw, err := json.Marshal(report)
	if err != nil {
		t.Fatalf("Marshal(inventory): %v", err)
	}
	if strings.Contains(string(raw), secret) {
		t.Fatal("inventory JSON leaks a plaintext env value")
	}
	if !strings.Contains(string(raw), "ANTHROPIC_API_KEY") {
		t.Fatal("inventory JSON missing the env variable name")
	}
}

// writeDetailFixture writes a config whose servers carry url and env
// detail. SetServer only writes command/args, so the raw file is written
// per backend format instead.
func writeDetailFixture(t *testing.T, h Harness, path string, _ Document) {
	t.Helper()
	var data []byte
	if h.Format == FormatTOML {
		data = []byte(`[mcp_servers.web]
command = "uvx"
args = ["mcp-server", "--port", "9000"]

[mcp_servers.remote]
url = "http://localhost:8080/mcp"

[mcp_servers.remote.env]
API_KEY = "v0"
TOKEN = "t0"
`)
	} else {
		data = []byte(`{"mcpServers":{"web":{"command":"uvx","args":["mcp-server","--port","9000"]},"remote":{"url":"http://localhost:8080/mcp","env":{"TOKEN":"t0","API_KEY":"v0"}}}}`)
	}
	if err := os.WriteFile(path, data, 0o600); err != nil {
		t.Fatalf("WriteFile(): %v", err)
	}
}

func serverNames(servers []ServerInfo) []string {
	names := make([]string, 0, len(servers))
	for _, s := range servers {
		names = append(names, s.Name)
	}
	return names
}

func serverInfoMap(servers []ServerInfo) map[string]ServerInfo {
	out := make(map[string]ServerInfo, len(servers))
	for _, s := range servers {
		out[s.Name] = s
	}
	return out
}

func TestList_ProjectLocalConfig(t *testing.T) {
	t.Setenv("HOME", t.TempDir())
	projectDir := t.TempDir()
	h := mustLookup(t, "claude")
	path := h.ProjectConfigPath(projectDir)
	doc := Empty(h)
	doc.SetServer("project-server", Entry{Command: "project-server"})
	data, err := doc.Marshal()
	if err != nil {
		t.Fatalf("Marshal(): %v", err)
	}
	if err := os.WriteFile(path, data, 0o600); err != nil {
		t.Fatalf("WriteFile(): %v", err)
	}

	item := findInventory(t, List(projectDir), h.Name)
	if item.Project == nil {
		t.Fatal("project inventory is nil")
	}
	if !item.Project.Exists || !item.Project.Parsed {
		t.Fatalf("project state = %+v, want existing and parsed", *item.Project)
	}
	if got, want := serverNames(item.Project.Servers), []string{"project-server"}; !reflect.DeepEqual(got, want) {
		t.Fatalf("project servers = %v, want %v", got, want)
	}
}

func TestList_MalformedConfig(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	h := mustLookup(t, "gemini")
	path, err := h.ConfigPath()
	if err != nil {
		t.Fatalf("ConfigPath(): %v", err)
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatalf("MkdirAll(): %v", err)
	}
	if err := os.WriteFile(path, []byte("{"), 0o600); err != nil {
		t.Fatalf("WriteFile(): %v", err)
	}

	item := findInventory(t, List(""), h.Name)
	if !item.Global.Exists || item.Global.Parsed {
		t.Fatalf("global state = %+v, want existing and malformed", item.Global)
	}
	if item.Global.Error == "" {
		t.Fatal("malformed config has no error")
	}
}

func mustLookup(t *testing.T, name string) Harness {
	t.Helper()
	h, err := Lookup(name)
	if err != nil {
		t.Fatalf("Lookup(%q): %v", name, err)
	}
	return h
}

func findInventory(t *testing.T, report Inventory, name Name) HarnessInventory {
	t.Helper()
	for _, item := range report.Harnesses {
		if item.Name == name {
			return item
		}
	}
	t.Fatalf("inventory has no %q entry", name)
	return HarnessInventory{}
}
