package harness

import (
	"os"
	"path/filepath"
	"reflect"
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
	if got, want := item.Global.Servers, []string{"alpha", "zeta"}; !reflect.DeepEqual(got, want) {
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
	if got, want := item.Project.Servers, []string{"project-server"}; !reflect.DeepEqual(got, want) {
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
