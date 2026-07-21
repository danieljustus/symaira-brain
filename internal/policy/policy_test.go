package policy

import (
	"reflect"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/profile"
)

func TestEvaluate_VaultModes(t *testing.T) {
	live := append(append([]string{}, KnownTools(profile.ServerVault)...), "vault_new_upstream_tool")

	tests := []struct {
		name        string
		cfg         profile.ServerConfig
		wantExposed []string
		wantUnknown []string
	}{
		{
			name:        "request_only exposes only the request-shaped triad",
			cfg:         profile.ServerConfig{Enabled: true, Mode: profile.VaultModeRequestOnly},
			wantExposed: []string{"generate_password", "health", "request_credential"},
			wantUnknown: []string{"vault_new_upstream_tool"},
		},
		{
			name:        "full exposes everything known",
			cfg:         profile.ServerConfig{Enabled: true, Mode: profile.VaultModeFull},
			wantExposed: KnownTools(profile.ServerVault),
			wantUnknown: []string{"vault_new_upstream_tool"},
		},
		{
			name:        "off exposes nothing",
			cfg:         profile.ServerConfig{Enabled: true, Mode: profile.VaultModeOff},
			wantExposed: []string{},
			wantUnknown: []string{"vault_new_upstream_tool"},
		},
		{
			name:        "disabled exposes nothing regardless of mode",
			cfg:         profile.ServerConfig{Enabled: false, Mode: profile.VaultModeFull},
			wantExposed: []string{},
			wantUnknown: []string{"vault_new_upstream_tool"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			report, err := Evaluate(profile.ServerVault, tt.cfg, live)
			if err != nil {
				t.Fatalf("Evaluate() error = %v, want nil", err)
			}
			assertStringSet(t, "Exposed", report.Exposed, tt.wantExposed)
			assertStringSet(t, "Unknown", report.Unknown, tt.wantUnknown)
			// exposed ∪ hidden ∪ unknown must partition the live list exactly.
			assertPartition(t, live, report)
		})
	}
}

func TestEvaluate_MemoryModes(t *testing.T) {
	live := append(append([]string{}, KnownTools(profile.ServerMemory)...), "memory_new_upstream_tool")

	tests := []struct {
		name        string
		cfg         profile.ServerConfig
		wantExposed []string
	}{
		{
			name:        "read_only exposes only read-shaped tools",
			cfg:         profile.ServerConfig{Enabled: true, Mode: profile.MemoryModeReadOnly},
			wantExposed: []string{"entity_list", "entity_resolve", "graph_neighbors", "memory_get", "memory_list", "memory_search"},
		},
		{
			name:        "read_write exposes everything known",
			cfg:         profile.ServerConfig{Enabled: true, Mode: profile.MemoryModeReadWrite},
			wantExposed: KnownTools(profile.ServerMemory),
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			report, err := Evaluate(profile.ServerMemory, tt.cfg, live)
			if err != nil {
				t.Fatalf("Evaluate() error = %v, want nil", err)
			}
			assertStringSet(t, "Exposed", report.Exposed, tt.wantExposed)
			assertStringSet(t, "Unknown", report.Unknown, []string{"memory_new_upstream_tool"})
			assertPartition(t, live, report)
		})
	}
}

// TestEvaluate_UnknownUpstreamToolNeverExposedUnderPreset is the key
// regression test from the issue: a tool the live child reports that is
// absent from this package's versioned preset list must never end up in
// Exposed, for any mode, even the most permissive one.
func TestEvaluate_UnknownUpstreamToolNeverExposedUnderPreset(t *testing.T) {
	unknown := "vault_totally_new_dangerous_tool"
	live := append(append([]string{}, KnownTools(profile.ServerVault)...), unknown)

	for _, mode := range []string{profile.VaultModeRequestOnly, profile.VaultModeFull} {
		t.Run(mode, func(t *testing.T) {
			report, err := Evaluate(profile.ServerVault, profile.ServerConfig{Enabled: true, Mode: mode}, live)
			if err != nil {
				t.Fatalf("Evaluate() error = %v, want nil", err)
			}
			for _, tool := range report.Exposed {
				if tool == unknown {
					t.Fatalf("Exposed contains %q, want it excluded under mode %q (default-deny for unknown tools)", unknown, mode)
				}
			}
			found := false
			for _, tool := range report.Unknown {
				if tool == unknown {
					found = true
				}
			}
			if !found {
				t.Errorf("Unknown = %v, want it to contain %q", report.Unknown, unknown)
			}
		})
	}
}

func TestEvaluate_ToolsAllowOverridesPreset(t *testing.T) {
	live := KnownTools(profile.ServerMemory)

	cfg := profile.ServerConfig{
		Enabled:    true,
		Mode:       profile.MemoryModeReadOnly, // would normally exclude memory_set
		ToolsAllow: []string{"memory_set"},
	}

	report, err := Evaluate(profile.ServerMemory, cfg, live)
	if err != nil {
		t.Fatalf("Evaluate() error = %v, want nil", err)
	}
	assertStringSet(t, "Exposed", report.Exposed, []string{"memory_set"})
	assertPartition(t, live, report)
}

func TestEvaluate_ToolsDenyRemovesFromPreset(t *testing.T) {
	live := KnownTools(profile.ServerVault)

	cfg := profile.ServerConfig{
		Enabled:   true,
		Mode:      profile.VaultModeFull,
		ToolsDeny: []string{"set_entry_field"},
	}

	report, err := Evaluate(profile.ServerVault, cfg, live)
	if err != nil {
		t.Fatalf("Evaluate() error = %v, want nil", err)
	}
	for _, tool := range report.Exposed {
		if tool == "set_entry_field" {
			t.Fatalf("Exposed contains %q, want it removed by tools_deny", tool)
		}
	}
	found := false
	for _, tool := range report.Hidden {
		if tool == "set_entry_field" {
			found = true
		}
	}
	if !found {
		t.Errorf("Hidden = %v, want it to contain %q", report.Hidden, "set_entry_field")
	}
	assertPartition(t, live, report)
}

func TestEvaluate_DenyWinsOverAllow(t *testing.T) {
	live := []string{"memory_set", "memory_search"}

	cfg := profile.ServerConfig{
		Enabled:    true,
		Mode:       profile.MemoryModeReadWrite,
		ToolsAllow: []string{"memory_set", "memory_search"},
		ToolsDeny:  []string{"memory_set"},
	}

	report, err := Evaluate(profile.ServerMemory, cfg, live)
	if err != nil {
		t.Fatalf("Evaluate() error = %v, want nil", err)
	}
	assertStringSet(t, "Exposed", report.Exposed, []string{"memory_search"})
	assertStringSet(t, "Hidden", report.Hidden, []string{"memory_set"})
}

func TestEvaluate_Skills(t *testing.T) {
	tests := []struct {
		name        string
		cfg         profile.ServerConfig
		live        []string
		wantExposed []string
		wantHidden  []string
	}{
		{
			name:        "enabled with no allow/deny exposes everything live (always-full)",
			cfg:         profile.ServerConfig{Enabled: true},
			live:        []string{"skill_render", "skill_install", "skill_list"},
			wantExposed: []string{"skill_install", "skill_list", "skill_render"},
		},
		{
			name:        "disabled exposes nothing",
			cfg:         profile.ServerConfig{Enabled: false},
			live:        []string{"skill_render", "skill_install"},
			wantExposed: []string{},
			wantHidden:  []string{"skill_install", "skill_render"},
		},
		{
			name:        "tools_deny narrows the always-full default",
			cfg:         profile.ServerConfig{Enabled: true, ToolsDeny: []string{"skill_install"}},
			live:        []string{"skill_render", "skill_install"},
			wantExposed: []string{"skill_render"},
			wantHidden:  []string{"skill_install"},
		},
		{
			name:        "tools_allow replaces the always-full default",
			cfg:         profile.ServerConfig{Enabled: true, ToolsAllow: []string{"skill_render"}},
			live:        []string{"skill_render", "skill_install"},
			wantExposed: []string{"skill_render"},
			wantHidden:  []string{"skill_install"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			report, err := Evaluate(profile.ServerSkills, tt.cfg, tt.live)
			if err != nil {
				t.Fatalf("Evaluate() error = %v, want nil", err)
			}
			assertStringSet(t, "Exposed", report.Exposed, tt.wantExposed)
			if tt.wantHidden != nil {
				assertStringSet(t, "Hidden", report.Hidden, tt.wantHidden)
			}
			// Skills has no bounded universe: nothing is ever "unknown".
			if len(report.Unknown) != 0 {
				t.Errorf("Unknown = %v, want empty (skills has no preset universe)", report.Unknown)
			}
		})
	}
}

func TestEvaluate_UnknownServerAliasErrors(t *testing.T) {
	_, err := Evaluate("nope", profile.ServerConfig{Enabled: true}, nil)
	if err == nil {
		t.Fatal("Evaluate() error = nil, want error for unknown alias")
	}
}

func TestEvaluatePreset_VaultAndMemory(t *testing.T) {
	report, err := EvaluatePreset(profile.ServerVault, profile.ServerConfig{Enabled: true, Mode: profile.VaultModeRequestOnly})
	if err != nil {
		t.Fatalf("EvaluatePreset() error = %v, want nil", err)
	}
	assertStringSet(t, "Exposed", report.Exposed, []string{"generate_password", "health", "request_credential"})
	if len(report.Unknown) != 0 {
		t.Errorf("Unknown = %v, want empty (preset preview has no live catalog surprises)", report.Unknown)
	}

	report, err = EvaluatePreset(profile.ServerMemory, profile.ServerConfig{Enabled: true, Mode: profile.MemoryModeReadWrite})
	if err != nil {
		t.Fatalf("EvaluatePreset() error = %v, want nil", err)
	}
	assertStringSet(t, "Exposed", report.Exposed, KnownTools(profile.ServerMemory))
}

func TestEvaluatePreset_SkillsUnsupported(t *testing.T) {
	_, err := EvaluatePreset(profile.ServerSkills, profile.ServerConfig{Enabled: true})
	if err == nil {
		t.Fatal("EvaluatePreset(skills) error = nil, want error (skills has no mode preset)")
	}
}

func TestPresetTools(t *testing.T) {
	tools, err := PresetTools(profile.ServerVault, profile.VaultModeOff)
	if err != nil {
		t.Fatalf("PresetTools() error = %v, want nil", err)
	}
	if len(tools) != 0 {
		t.Errorf("PresetTools(vault, off) = %v, want empty", tools)
	}

	if _, err := PresetTools(profile.ServerVault, "not-a-real-mode"); err == nil {
		t.Error("PresetTools() error = nil, want error for unknown mode")
	}
}

func TestReport_VerdictLookup(t *testing.T) {
	live := []string{"memory_search", "memory_set", "totally_new"}
	cfg := profile.ServerConfig{Enabled: true, Mode: profile.MemoryModeReadOnly}

	report, err := Evaluate(profile.ServerMemory, cfg, live)
	if err != nil {
		t.Fatalf("Evaluate() error = %v, want nil", err)
	}

	if got := report.Verdict("memory_search"); got != Exposed {
		t.Errorf("Verdict(memory_search) = %q, want %q", got, Exposed)
	}
	if got := report.Verdict("memory_set"); got != Hidden {
		t.Errorf("Verdict(memory_set) = %q, want %q", got, Hidden)
	}
	if got := report.Verdict("totally_new"); got != Unknown {
		t.Errorf("Verdict(totally_new) = %q, want %q", got, Unknown)
	}
	if got := report.Verdict("never_seen"); got != Unknown {
		t.Errorf("Verdict(never_seen) = %q, want %q (not even live)", got, Unknown)
	}
}

// assertStringSet compares got and want as sets (order-independent) after
// sorting, and fails with a readable diff otherwise. Both may be nil/empty.
func assertStringSet(t *testing.T, label string, got, want []string) {
	t.Helper()
	g := append([]string{}, got...)
	w := append([]string{}, want...)
	sortStrings(g)
	sortStrings(w)
	if !reflect.DeepEqual(g, w) {
		t.Errorf("%s = %v, want %v", label, got, want)
	}
}

func sortStrings(s []string) {
	for i := 1; i < len(s); i++ {
		for j := i; j > 0 && s[j-1] > s[j]; j-- {
			s[j-1], s[j] = s[j], s[j-1]
		}
	}
}

// assertPartition verifies every live tool appears in exactly one of
// Exposed/Hidden/Unknown, and nothing extra shows up.
func assertPartition(t *testing.T, live []string, report *Report) {
	t.Helper()
	seen := map[string]int{}
	for _, tool := range report.Exposed {
		seen[tool]++
	}
	for _, tool := range report.Hidden {
		seen[tool]++
	}
	for _, tool := range report.Unknown {
		seen[tool]++
	}
	for _, tool := range live {
		if seen[tool] != 1 {
			t.Errorf("tool %q appears in %d of Exposed/Hidden/Unknown buckets, want exactly 1", tool, seen[tool])
		}
	}
	if len(seen) != len(live) {
		t.Errorf("report buckets mention %d distinct tools, want exactly the %d live tools", len(seen), len(live))
	}
}
