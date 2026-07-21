package audit

import (
	"encoding/json"
	"testing"
)

func TestRedactArgs_VaultNeverLogsAnything(t *testing.T) {
	tests := []struct {
		server string
		tool   string
		args   string
	}{
		{"vault", "get_entry", `{"id":"secret123","path":"creds/api-key"}`},
		{"vault", "vault_get_entry", `{"password":"hunter2"}`},
		{"vault", "find_entries", `{"query":"api_keys"}`},
		{"vault", "set_entry_field", `{"path":"x","field":"token","value":"super-secret"}`},
		{"vault", "request_credential", `{"path":"x","field":"pw","reason":"need it"}`},
	}

	for _, tt := range tests {
		keys, values := redactArgs(tt.server, tt.tool, json.RawMessage(tt.args), false)
		if keys != "" {
			t.Errorf("vault tool %q: keys = %q, want empty (vault args must never be logged)", tt.tool, keys)
		}
		if values != "" {
			t.Errorf("vault tool %q: values = %q, want empty (vault values must never be logged)", tt.tool, values)
		}
	}
}

func TestRedactArgs_VaultNeverLogsEvenWithVerbose(t *testing.T) {
	args := `{"id":"secret-id","password":"hunter2"}`
	keys, values := redactArgs("vault", "get_entry", json.RawMessage(args), true)
	if keys != "" {
		t.Errorf("vault verbose: keys = %q, want empty", keys)
	}
	if values != "" {
		t.Errorf("vault verbose: values = %q, want empty", values)
	}
}

func TestRedactArgs_NonVaultKeysOnlyByDefault(t *testing.T) {
	args := `{"query":"search term","limit":10}`
	keys, values := redactArgs("memory", "memory_search", json.RawMessage(args), false)

	if keys == "" {
		t.Error("non-vault default: keys should not be empty")
	}
	if values != "" {
		t.Errorf("non-vault default: values = %q, want empty", values)
	}
}

func TestRedactArgs_NonVaultVerboseIncludesValues(t *testing.T) {
	args := `{"query":"search term","limit":10}`
	keys, values := redactArgs("memory", "memory_search", json.RawMessage(args), true)

	if keys == "" {
		t.Error("non-vault verbose: keys should not be empty")
	}
	if values == "" {
		t.Error("non-vault verbose: values should not be empty")
	}
}

func TestRedactArgs_EmptyArgs(t *testing.T) {
	keys, values := redactArgs("vault", "health", nil, false)
	if keys != "" || values != "" {
		t.Errorf("nil args: keys=%q values=%q, want both empty", keys, values)
	}

	keys, values = redactArgs("memory", "memory_search", json.RawMessage(`{}`), false)
	if keys != "" || values != "" {
		t.Errorf("empty object: keys=%q values=%q, want both empty", keys, values)
	}
}

func TestRedactArgs_InvalidJSON(t *testing.T) {
	keys, values := redactArgs("memory", "memory_search", json.RawMessage(`not json`), false)
	if keys != "" || values != "" {
		t.Errorf("invalid JSON: keys=%q values=%q, want both empty", keys, values)
	}
}

func TestRedactArgs_MemoryEntityGraphToolsNotVault(t *testing.T) {
	tests := []struct {
		server string
		tool   string
	}{
		{"memory", "entity_list"},
		{"memory", "graph_neighbors"},
		{"memory", "entity_relate"},
	}

	for _, tt := range tests {
		args := `{"name":"Alice"}`
		keys, _ := redactArgs(tt.server, tt.tool, json.RawMessage(args), false)
		if keys == "" {
			t.Errorf("%s/%s: keys should not be empty (not a vault tool)", tt.server, tt.tool)
		}
	}
}

func TestEntry_JSON(t *testing.T) {
	entry := Entry{
		Timestamp:  "2026-01-01T00:00:00Z",
		Profile:    "personal",
		Server:     "vault",
		Tool:       "get_entry",
		DurationMS: 42,
		Status:     "ok",
	}

	data, err := json.Marshal(entry)
	if err != nil {
		t.Fatalf("Marshal: %v", err)
	}

	var decoded Entry
	if err := json.Unmarshal(data, &decoded); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}

	if decoded.Timestamp != entry.Timestamp {
		t.Errorf("Timestamp = %q, want %q", decoded.Timestamp, entry.Timestamp)
	}
	if decoded.Server != entry.Server {
		t.Errorf("Server = %q, want %q", decoded.Server, entry.Server)
	}
	if decoded.Tool != entry.Tool {
		t.Errorf("Tool = %q, want %q", decoded.Tool, entry.Tool)
	}
}

func TestRedactArgs_KnownVaultPrefixes(t *testing.T) {
	prefixes := []string{"vault_", "get_entry", "find_entries", "set_entry_field", "symaira_"}
	for _, prefix := range prefixes {
		args := `{"key":"value"}`
		keys, values := redactArgs("vault", prefix+"test", json.RawMessage(args), true)
		if keys != "" || values != "" {
			t.Errorf("vault prefix %q: keys=%q values=%q, want both empty", prefix, keys, values)
		}
	}
}
