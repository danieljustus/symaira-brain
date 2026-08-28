package policy

import (
	"testing"

	"github.com/danieljustus/symaira-brain/internal/profile"
)

func TestMemoryActivityReadsDefaultDenyAndExplicitAllow(t *testing.T) {
	cfg := profile.ServerConfig{Enabled: true, Mode: profile.MemoryModeReadOnly}
	report, err := EvaluatePreset(profile.ServerMemory, cfg)
	if err != nil {
		t.Fatal(err)
	}
	for _, tool := range []string{"activity_search", "activity_get", "activity_status"} {
		if report.Verdict(tool) != Hidden {
			t.Fatalf("default read-only profile exposed %s: %+v", tool, report)
		}
	}
	cfg.ToolsAllow = []string{"activity_search", "activity_get", "activity_status"}
	report, err = EvaluatePreset(profile.ServerMemory, cfg)
	if err != nil {
		t.Fatal(err)
	}
	if got := report.Exposed; len(got) != 3 {
		t.Fatalf("explicit activity grant exposed %v", got)
	}
}
