package catalog

import (
	"encoding/json"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/policy"
	"github.com/danieljustus/symaira-brain/internal/profile"
)

func toolList(names ...string) []Tool {
	tools := make([]Tool, len(names))
	for i, name := range names {
		tools[i] = Tool{Name: name, Description: "desc for " + name}
	}
	return tools
}

func fullReport(server string) *policy.Report {
	cfg := profile.ServerConfig{Enabled: true, Mode: profile.VaultModeFull}
	if server == "memory" {
		cfg.Mode = profile.MemoryModeReadWrite
	}
	r, _ := policy.Evaluate(server, cfg, policy.KnownTools(server))
	return r
}

func requestOnlyReport() *policy.Report {
	cfg := profile.ServerConfig{Enabled: true, Mode: profile.VaultModeRequestOnly}
	r, _ := policy.Evaluate("vault", cfg, policy.KnownTools("vault"))
	return r
}

func TestBuild_VaultPrefix(t *testing.T) {
	c, err := Build([]ServerTools{
		{
			Server: "vault",
			Tools:  toolList("get_entry", "health"),
			Report: fullReport("vault"),
		},
	})
	if err != nil {
		t.Fatalf("Build() error = %v", err)
	}

	names := c.Names()
	for _, want := range []string{"vault_get_entry", "vault_health"} {
		found := false
		for _, n := range names {
			if n == want {
				found = true
				break
			}
		}
		if !found {
			t.Errorf("Names() missing %q, got %v", want, names)
		}
	}
}

func TestBuild_MemoryPassthrough(t *testing.T) {
	c, err := Build([]ServerTools{
		{
			Server: "memory",
			Tools:  toolList("memory_search", "entity_list"),
			Report: fullReport("memory"),
		},
	})
	if err != nil {
		t.Fatalf("Build() error = %v", err)
	}

	names := c.Names()
	for _, want := range []string{"memory_search", "entity_list"} {
		found := false
		for _, n := range names {
			if n == want {
				found = true
				break
			}
		}
		if !found {
			t.Errorf("Names() missing %q, got %v", want, names)
		}
	}
}

func TestBuild_AlreadyPrefixed(t *testing.T) {
	c, err := Build([]ServerTools{
		{
			Server: "memory",
			Tools:  toolList("memory_search", "entity_list", "graph_neighbors"),
			Report: fullReport("memory"),
		},
	})
	if err != nil {
		t.Fatalf("Build() error = %v", err)
	}

	names := c.Names()
	for _, want := range []string{"memory_search", "entity_list", "graph_neighbors"} {
		found := false
		for _, n := range names {
			if n == want {
				found = true
				break
			}
		}
		if !found {
			t.Errorf("Names() missing already-prefixed %q, got %v", want, names)
		}
	}
}

func TestBuild_CollisionError(t *testing.T) {
	c, err := Build([]ServerTools{
		{
			Server: "vault",
			Tools:  toolList("vault_shared_tool"),
			Report: fullReport("vault"),
		},
		{
			Server: "memory",
			Tools:  toolList("vault_shared_tool"),
			Report: fullReport("memory"),
		},
	})
	if c != nil {
		t.Fatalf("Build() returned non-nil catalog on collision")
	}
	if err == nil {
		t.Fatal("Build() error = nil, want CollisionError")
	}
	var colErr *CollisionError
	if !isCollisionError(err, &colErr) {
		t.Fatalf("error = %v (%T), want *CollisionError", err, err)
	}
	if colErr.Name != "vault_shared_tool" {
		t.Errorf("CollisionError.Name = %q, want vault_shared_tool", colErr.Name)
	}
}

func TestBuild_DeterministicOrdering(t *testing.T) {
	tools := []string{"zebra", "alpha", "middle"}
	c1, err := Build([]ServerTools{
		{Server: "vault", Tools: toolList(tools...), Report: fullReport("vault")},
	})
	if err != nil {
		t.Fatalf("Build() error = %v", err)
	}
	c2, err := Build([]ServerTools{
		{Server: "vault", Tools: toolList(tools...), Report: fullReport("vault")},
	})
	if err != nil {
		t.Fatalf("Build() error = %v", err)
	}

	names1 := c1.Names()
	names2 := c2.Names()
	if len(names1) != len(names2) {
		t.Fatalf("len mismatch: %d vs %d", len(names1), len(names2))
	}
	for i := range names1 {
		if names1[i] != names2[i] {
			t.Errorf("non-deterministic: run1[%d]=%q, run2[%d]=%q", i, names1[i], i, names2[i])
		}
	}
}

func TestBuild_SchemaPassthrough(t *testing.T) {
	schema := json.RawMessage(`{"type":"object","properties":{"id":{"type":"string"}}}`)
	c, err := Build([]ServerTools{
		{
			Server: "vault",
			Tools:  []Tool{{Name: "get_entry", Description: "fetch", InputSchema: schema}},
			Report: fullReport("vault"),
		},
	})
	if err != nil {
		t.Fatalf("Build() error = %v", err)
	}

	entry := c.Lookup("vault_get_entry")
	if entry == nil {
		t.Fatal("Lookup(vault_get_entry) = nil")
	}
	if string(entry.InputSchema) != string(schema) {
		t.Errorf("InputSchema = %s, want %s", entry.InputSchema, schema)
	}
}

func TestBuild_PolicyFiltering(t *testing.T) {
	c, err := Build([]ServerTools{
		{
			Server: "vault",
			Tools:  toolList("get_entry", "health", "request_credential"),
			Report: requestOnlyReport(),
		},
	})
	if err != nil {
		t.Fatalf("Build() error = %v", err)
	}

	exposed := c.Exposed()
	exposedNames := make(map[string]bool)
	for _, e := range exposed {
		exposedNames[e.Name] = true
	}

	if exposedNames["vault_get_entry"] {
		t.Error("vault_get_entry should be hidden in request_only mode")
	}
	if !exposedNames["vault_health"] {
		t.Error("vault_health should be exposed in request_only mode")
	}
	if !exposedNames["vault_request_credential"] {
		t.Error("vault_request_credential should be exposed in request_only mode")
	}
}

func TestNamespace_AlreadyPrefixed(t *testing.T) {
	tests := []struct {
		name   string
		prefix string
		want   string
	}{
		{"vault_get_entry", "vault_", "vault_get_entry"},
		{"memory_search", "vault_", "memory_search"},
		{"entity_list", "vault_", "entity_list"},
		{"graph_neighbors", "vault_", "graph_neighbors"},
		{"get_entry", "vault_", "vault_get_entry"},
		{"search", "", "search"},
	}
	for _, tt := range tests {
		got := namespace(tt.name, tt.prefix)
		if got != tt.want {
			t.Errorf("namespace(%q, %q) = %q, want %q", tt.name, tt.prefix, got, tt.want)
		}
	}
}

func isCollisionError(err error, target **CollisionError) bool {
	if e, ok := err.(*CollisionError); ok {
		*target = e
		return true
	}
	return false
}
