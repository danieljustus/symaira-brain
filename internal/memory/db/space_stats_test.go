package db

import (
	"testing"
	"time"
)

func TestSpaceIdentityMatches(t *testing.T) {
	a := SpaceIdentity{Model: "nomic-embed-text", Quantization: "int8", Dim: 768}
	tests := []struct {
		name string
		o    SpaceIdentity
		want bool
	}{
		{"identical", SpaceIdentity{Model: "nomic-embed-text", Quantization: "int8", Dim: 768}, true},
		{"model differs", SpaceIdentity{Model: "other", Quantization: "int8", Dim: 768}, false},
		{"quantization differs", SpaceIdentity{Model: "nomic-embed-text", Quantization: "int4", Dim: 768}, false},
		{"empty vs explicit quantization", SpaceIdentity{Model: "nomic-embed-text", Quantization: "", Dim: 768}, false},
		{"dim differs", SpaceIdentity{Model: "nomic-embed-text", Quantization: "int8", Dim: 512}, false},
		{"zero value never matches populated", SpaceIdentity{}, false},
		{"zero value matches zero value", SpaceIdentity{}, true},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			receiver := a
			if tt.name == "zero value matches zero value" {
				receiver = SpaceIdentity{}
			}
			if got := receiver.Matches(tt.o); got != tt.want {
				t.Errorf("Matches(%+v) = %v, want %v", tt.o, got, tt.want)
			}
		})
	}
}

func TestSpaceIdentityString(t *testing.T) {
	tests := []struct {
		name string
		s    SpaceIdentity
		want string
	}{
		{"quantized", SpaceIdentity{Model: "m", Quantization: "int8", Dim: 768}, "m/int8/dim768"},
		{"legacy unquantized", SpaceIdentity{Model: "m", Quantization: "", Dim: 768}, "m/unquantized/dim768"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := tt.s.String(); got != tt.want {
				t.Errorf("String() = %q, want %q", got, tt.want)
			}
		})
	}
}

func TestRetrievalStatsSnapshot(t *testing.T) {
	rs := &RetrievalStats{}
	rs.Record(3, 0.9, 10*time.Millisecond)
	rs.Record(0, 0, 5*time.Millisecond) // zero-result query, no score contribution
	rs.Record(1, 0.5, 20*time.Millisecond)

	snap := rs.Snapshot()
	if snap.TotalQueries != 3 {
		t.Errorf("TotalQueries = %d, want 3", snap.TotalQueries)
	}
	if snap.ZeroResultQueries != 1 {
		t.Errorf("ZeroResultQueries = %d, want 1", snap.ZeroResultQueries)
	}
	if snap.ScoreMin != 0.5 {
		t.Errorf("ScoreMin = %v, want 0.5", snap.ScoreMin)
	}
	if snap.ScoreMax != 0.9 {
		t.Errorf("ScoreMax = %v, want 0.9", snap.ScoreMax)
	}
	if snap.ScoreAvg != 0.7 {
		t.Errorf("ScoreAvg = %v, want 0.7", snap.ScoreAvg)
	}
	if snap.AvgLatencyMs <= 0 {
		t.Errorf("AvgLatencyMs = %v, want > 0", snap.AvgLatencyMs)
	}
}

func TestRetrievalStatsSnapshotEmpty(t *testing.T) {
	rs := &RetrievalStats{}
	snap := rs.Snapshot()
	if snap.TotalQueries != 0 || snap.ZeroResultQueries != 0 {
		t.Errorf("empty snapshot should be zeroed, got %+v", snap)
	}
	if snap.ScoreAvg != 0 {
		t.Errorf("ScoreAvg on empty stats = %v, want 0", snap.ScoreAvg)
	}
}

func TestRetrievalStatsReset(t *testing.T) {
	rs := &RetrievalStats{}
	rs.Record(5, 0.8, 15*time.Millisecond)
	rs.Record(2, 0.6, 25*time.Millisecond)

	rs.Reset()
	snap := rs.Snapshot()
	if snap.TotalQueries != 0 || snap.ZeroResultQueries != 0 {
		t.Errorf("after Reset stats should be zeroed, got %+v", snap)
	}
	if snap.ScoreMin != 0 || snap.ScoreMax != 0 || snap.ScoreAvg != 0 {
		t.Errorf("after Reset scores should be zeroed, got min=%v max=%v avg=%v", snap.ScoreMin, snap.ScoreMax, snap.ScoreAvg)
	}
	if snap.AvgLatencyMs != 0 {
		t.Errorf("after Reset AvgLatencyMs = %v, want 0", snap.AvgLatencyMs)
	}

	// Stats must be usable after reset.
	rs.Record(1, 0.4, 5*time.Millisecond)
	snap2 := rs.Snapshot()
	if snap2.TotalQueries != 1 || snap2.ScoreMax != 0.4 {
		t.Errorf("stats unusable after reset: %+v", snap2)
	}
}
