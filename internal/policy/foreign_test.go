package policy

import (
	"testing"

	"github.com/danieljustus/symaira-brain/internal/profile"
)

// ---- helpers ----

func boolPtr(b bool) *bool { return &b }

func ro(name string, readOnly *bool) ForeignTool {
	return ForeignTool{Name: name, ReadOnlyHint: readOnly}
}

func plain(names ...string) []ForeignTool {
	tools := make([]ForeignTool, len(names))
	for i, n := range names {
		tools[i] = ForeignTool{Name: n}
	}
	return tools
}

func foreignCfg(access string, overrides map[string]string) profile.ServerConfig {
	cfg := profile.ServerConfig{Enabled: true, Access: access}
	for name, class := range overrides {
		if class == "read" {
			cfg.ToolsRead = append(cfg.ToolsRead, name)
		} else {
			cfg.ToolsWrite = append(cfg.ToolsWrite, name)
		}
	}
	return cfg
}

func exposed(report *Report) map[string]bool {
	set := make(map[string]bool, len(report.Exposed))
	for _, t := range report.Exposed {
		set[t] = true
	}
	return set
}

func wantExposed(t *testing.T, report *Report, names ...string) {
	t.Helper()
	set := exposed(report)
	for _, n := range names {
		if !set[n] {
			t.Errorf("tool %q should be exposed; exposed=%v", n, report.Exposed)
		}
	}
	if len(set) != len(names) {
		t.Errorf("exposed set = %v, want exactly %v", report.Exposed, names)
	}
}

func wantHidden(t *testing.T, report *Report, names ...string) {
	t.Helper()
	for _, n := range names {
		for _, e := range report.Exposed {
			if e == n {
				t.Errorf("tool %q should be hidden; exposed=%v", n, report.Exposed)
			}
		}
	}
}

// ---- tests ----

func TestEvaluateForeign_AccessReadExposesOnlyReadingTools(t *testing.T) {
	tools := []ForeignTool{ro("search", boolPtr(true)), ro("delete", boolPtr(false)), ro("unannotated", nil)}
	report, err := EvaluateForeign("fig", foreignCfg(profile.ForeignAccessRead, nil), tools)
	if err != nil {
		t.Fatalf("EvaluateForeign() error = %v", err)
	}
	wantExposed(t, report, "search")
	wantHidden(t, report, "delete", "unannotated")
	if got := report.Exposures["search"]; got.Class != "read" || got.Source != ExposureSourceReadOnlyHint {
		t.Errorf("search exposure = %+v, want read/read_only_hint", got)
	}
	if got := report.Exposures["delete"]; got.Class != "write" || got.Source != ExposureSourceDefaultWrite {
		t.Errorf("delete exposure = %+v, want write/default_write (no hint)", got)
	}
}

func TestEvaluateForeign_AccessWriteExposesEverything(t *testing.T) {
	tools := []ForeignTool{ro("search", boolPtr(true)), ro("delete", boolPtr(false))}
	report, err := EvaluateForeign("fig", foreignCfg(profile.ForeignAccessWrite, nil), tools)
	if err != nil {
		t.Fatalf("EvaluateForeign() error = %v", err)
	}
	wantExposed(t, report, "search", "delete")
}

func TestEvaluateForeign_ToolsReadOverridesWriteHint(t *testing.T) {
	// Explicit tools_read wins over the upstream readOnlyHint=false: the
	// tool counts as reading and survives access="read".
	cfg := foreignCfg(profile.ForeignAccessRead, map[string]string{"search": "read"})
	tools := []ForeignTool{ro("search", boolPtr(false))}
	report, err := EvaluateForeign("fig", cfg, tools)
	if err != nil {
		t.Fatalf("EvaluateForeign() error = %v", err)
	}
	wantExposed(t, report, "search")
	if got := report.Exposures["search"]; got.Source != ExposureSourceToolsRead {
		t.Errorf("search source = %q, want tools_read", got.Source)
	}
}

func TestEvaluateForeign_ToolsWriteOverridesReadHint(t *testing.T) {
	// Explicit tools_write wins over the upstream readOnlyHint=true: the
	// tool counts as writing and is hidden under access="read".
	cfg := foreignCfg(profile.ForeignAccessRead, map[string]string{"search": "write"})
	tools := []ForeignTool{ro("search", boolPtr(true))}
	report, err := EvaluateForeign("fig", cfg, tools)
	if err != nil {
		t.Fatalf("EvaluateForeign() error = %v", err)
	}
	wantHidden(t, report, "search")
	if got := report.Exposures["search"]; got.Source != ExposureSourceToolsWrite {
		t.Errorf("search source = %q, want tools_write", got.Source)
	}
}

func TestEvaluateForeign_UnannotatedToolDefaultsToWrite(t *testing.T) {
	// No annotation, no profile entry → writing (exposed under write,
	// hidden under read).
	cfg := foreignCfg(profile.ForeignAccessRead, nil)
	report, err := EvaluateForeign("fig", cfg, plain("some_tool"))
	if err != nil {
		t.Fatalf("EvaluateForeign() error = %v", err)
	}
	wantHidden(t, report, "some_tool")
	if got := report.Exposures["some_tool"]; got.Class != "write" || got.Source != ExposureSourceDefaultWrite {
		t.Errorf("some_tool exposure = %+v, want write/default_write", got)
	}
}

func TestEvaluateForeign_ToolsDenyWinsOverAccessAndAllow(t *testing.T) {
	cfg := foreignCfg(profile.ForeignAccessWrite, nil)
	cfg.ToolsDeny = []string{"delete"}
	tools := []ForeignTool{ro("delete", boolPtr(true)), ro("search", boolPtr(true))}
	report, err := EvaluateForeign("fig", cfg, tools)
	if err != nil {
		t.Fatalf("EvaluateForeign() error = %v", err)
	}
	wantHidden(t, report, "delete") // deny wins even though read-only
	wantExposed(t, report, "search")
}

func TestEvaluateForeign_ToolsAllowNarrows(t *testing.T) {
	cfg := foreignCfg(profile.ForeignAccessWrite, nil)
	cfg.ToolsAllow = []string{"search"}
	tools := []ForeignTool{ro("search", boolPtr(true)), ro("other", boolPtr(true))}
	report, err := EvaluateForeign("fig", cfg, tools)
	if err != nil {
		t.Fatalf("EvaluateForeign() error = %v", err)
	}
	wantExposed(t, report, "search")
	wantHidden(t, report, "other")
}

func TestEvaluateForeign_DisabledHidesEverything(t *testing.T) {
	cfg := foreignCfg(profile.ForeignAccessWrite, nil)
	cfg.Enabled = false
	report, err := EvaluateForeign("fig", cfg, []ForeignTool{ro("search", boolPtr(true))})
	if err != nil {
		t.Fatalf("EvaluateForeign() error = %v", err)
	}
	if len(report.Exposed) != 0 {
		t.Errorf("exposed = %v, want none when disabled", report.Exposed)
	}
	wantHidden(t, report, "search")
}

func TestEvaluateForeign_InvalidAccessErrors(t *testing.T) {
	cfg := profile.ServerConfig{Enabled: true, Access: "exec"}
	if _, err := EvaluateForeign("fig", cfg, nil); err == nil {
		t.Fatal("EvaluateForeign() error = nil, want error for invalid access class")
	}
}

func TestEvaluateForeign_DefaultAccessIsWrite(t *testing.T) {
	// Empty access field defaults to write (exposed filter model).
	cfg := profile.ServerConfig{Enabled: true}
	report, err := EvaluateForeign("fig", cfg, plain("anything"))
	if err != nil {
		t.Fatalf("EvaluateForeign() error = %v", err)
	}
	wantExposed(t, report, "anything")
}
