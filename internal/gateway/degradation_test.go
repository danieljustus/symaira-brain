package gateway

import (
	"testing"

	"github.com/danieljustus/symaira-brain/internal/audit"
)

func TestInstructions_ReportsOnlyDegradedBackends(t *testing.T) {
	s := &Server{profile: testProfile()}
	if got, want := s.instructions(), `symbrain profile "test"`; got != want {
		t.Fatalf("healthy instructions = %q, want %q", got, want)
	}

	s.degradations = []audit.Degradation{
		{Server: "memory", Reason: "timeout", Level: "warning"},
		{Server: "vault", Reason: "closed", Level: "warning"},
	}
	got := s.instructions()
	want := `symbrain profile "test"; degraded backends: memory, vault`
	if got != want {
		t.Fatalf("degraded instructions = %q, want %q", got, want)
	}
}
