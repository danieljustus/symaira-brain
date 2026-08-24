package patterns

import (
	"strings"
	"testing"
)

func step(server, tool string) Step {
	return Step{Server: server, Tool: tool}
}

func episode(profile string, steps []Step) Episode {
	return Episode{
		Profile:   profile,
		Steps:     steps,
		StartedAt: "2026-08-01T09:00:00Z",
		EndedAt:   "2026-08-01T09:05:00Z",
	}
}

func TestPromote_Threshold(t *testing.T) {
	seq := []Step{step("memory", "memory_search"), step("vault", "request_credential")}

	tests := []struct {
		name      string
		episodes  []Episode
		threshold int
		want      int
	}{
		{
			name:      "below threshold not promoted",
			episodes:  []Episode{episode("p", seq), episode("p", seq)},
			threshold: 3,
			want:      0,
		},
		{
			name:      "at threshold promoted",
			episodes:  []Episode{episode("p", seq), episode("p", seq), episode("p", seq)},
			threshold: 3,
			want:      1,
		},
		{
			name: "single-step episodes never promoted",
			episodes: []Episode{
				episode("p", []Step{step("vault", "health")}),
				episode("p", []Step{step("vault", "health")}),
				episode("p", []Step{step("vault", "health")}),
			},
			threshold: 3,
			want:      0,
		},
		{
			name: "different profiles do not count together",
			episodes: []Episode{
				episode("p1", seq),
				episode("p1", seq),
				episode("p2", seq),
			},
			threshold: 3,
			want:      0,
		},
		{
			name: "same tools different order is a different sequence",
			episodes: []Episode{
				episode("p", seq),
				episode("p", seq),
				episode("p", []Step{step("vault", "request_credential"), step("memory", "memory_search")}),
			},
			threshold: 3,
			want:      0,
		},
		{
			name: "mixed recurring sequences both promoted",
			episodes: []Episode{
				episode("p", seq),
				episode("p", seq),
				episode("p", seq),
				episode("p", []Step{step("skills", "skills_list"), step("skills", "skills_install")}),
				episode("p", []Step{step("skills", "skills_list"), step("skills", "skills_install")}),
				episode("p", []Step{step("skills", "skills_list"), step("skills", "skills_install")}),
			},
			threshold: 3,
			want:      2,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := Promote(tt.episodes, tt.threshold)
			if len(got) != tt.want {
				t.Errorf("Promote() = %d patterns, want %d: %+v", len(got), tt.want, got)
			}
		})
	}
}

func TestPromote_ProvenanceAndTrigger(t *testing.T) {
	seq := []Step{step("memory", "memory_search"), step("vault", "request_credential")}
	episodes := []Episode{
		{Profile: "p", Steps: seq, StartedAt: "2026-08-01T09:00:00Z", EndedAt: "2026-08-01T09:05:00Z"},
		{Profile: "p", Steps: seq, StartedAt: "2026-08-02T09:00:00Z", EndedAt: "2026-08-02T09:05:00Z"},
		{Profile: "p", Steps: seq, StartedAt: "2026-08-03T09:00:00Z", EndedAt: "2026-08-03T09:05:00Z"},
	}

	got := Promote(episodes, 3)
	if len(got) != 1 {
		t.Fatalf("Promote() = %d patterns, want 1", len(got))
	}
	r := got[0]

	if r.Profile != "p" || r.Version != 1 {
		t.Errorf("Profile/Version = %q/%d, want p/1", r.Profile, r.Version)
	}
	if r.Trigger.Profile != "p" || r.Trigger.FirstStep != seq[0] {
		t.Errorf("Trigger = %+v, want profile p first step %+v", r.Trigger, seq[0])
	}
	if r.Provenance.RecurrenceCount != 3 {
		t.Errorf("RecurrenceCount = %d, want 3", r.Provenance.RecurrenceCount)
	}
	if r.Provenance.FirstSeenAt != "2026-08-01T09:00:00Z" || r.Provenance.LastSeenAt != "2026-08-03T09:05:00Z" {
		t.Errorf("Provenance window = %q..%q, want 08-01..08-03",
			r.Provenance.FirstSeenAt, r.Provenance.LastSeenAt)
	}
	if len(r.Steps) != 2 || r.Steps[0] != seq[0] || r.Steps[1] != seq[1] {
		t.Errorf("Steps = %+v, want the recorded sequence", r.Steps)
	}
}

func TestPromote_SortedByName(t *testing.T) {
	seqA := []Step{step("skills", "skills_list"), step("skills", "skills_install")}
	seqB := []Step{step("memory", "memory_search"), step("vault", "request_credential")}
	episodes := []Episode{
		episode("p", seqA), episode("p", seqA), episode("p", seqA),
		episode("p", seqB), episode("p", seqB), episode("p", seqB),
	}

	got := Promote(episodes, 3)
	if len(got) != 2 {
		t.Fatalf("Promote() = %d patterns, want 2", len(got))
	}
	if got[0].Name >= got[1].Name {
		t.Errorf("patterns not sorted by name: %q >= %q", got[0].Name, got[1].Name)
	}
}

func TestName_DeterministicAndDistinct(t *testing.T) {
	seq := []Step{step("vault", "request_credential"), step("memory", "memory_search")}

	a := Name("p", seq)
	b := Name("p", seq)
	if a != b {
		t.Errorf("Name() not deterministic: %q vs %q", a, b)
	}

	other := Name("p", []Step{step("vault", "request_credential"), step("memory", "entity_list")})
	if a == other {
		t.Error("Name() collision for different sequences")
	}

	if !strings.HasPrefix(a, "p_vault_request_credential_memory_memory_search_") {
		t.Errorf("Name() = %q, want readable prefix", a)
	}
	if strings.Contains(a, "/") || strings.Contains(a, " ") {
		t.Errorf("Name() = %q, want a safe slug", a)
	}
}
