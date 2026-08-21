package audit

import (
	"encoding/json"
	"testing"
)

func TestBundleSchemaVersion(t *testing.T) {
	if BundleSchemaVersion != 1 {
		t.Errorf("BundleSchemaVersion = %d, want 1", BundleSchemaVersion)
	}
}

func TestNewManifest(t *testing.T) {
	m := NewManifest("case-001")
	if m.CaseID != "case-001" {
		t.Errorf("CaseID = %q, want case-001", m.CaseID)
	}
	if m.SchemaVersion != 1 {
		t.Errorf("SchemaVersion = %d, want 1", m.SchemaVersion)
	}
	if !m.Unsigned {
		t.Error("Unsigned should be true for new manifests")
	}
	if m.RecordCounts == nil {
		t.Error("RecordCounts should be initialized")
	}
	if m.Digests == nil {
		t.Error("Digests should be initialized")
	}
}

func TestEvidenceRef(t *testing.T) {
	tests := []struct {
		name string
		ref  EvidenceRef
		want string
	}{
		{"file ref", "ref://main.go:42", "ref://main.go:42"},
		{"event ref", "ref://event/evt_001", "ref://event/evt_001"},
		{"hash ref", "ref://hash/abc123", "ref://hash/abc123"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if string(tt.ref) != tt.want {
				t.Errorf("EvidenceRef = %q, want %q", tt.ref, tt.want)
			}
		})
	}
}

func TestEventRecord(t *testing.T) {
	evt := Event{
		ID:         "evt_001",
		SchemaVer:  1,
		EventType:  "tool_call",
		AgentID:    "test-agent",
		Server:     "filesystem",
		Tool:       "read_file",
		Capability: "read_private",
		Decision:   "allow",
		Redacted:   true,
		Timestamp:  "2026-07-30T12:00:00Z",
	}
	if evt.ID != "evt_001" {
		t.Errorf("Event ID = %q, want evt_001", evt.ID)
	}
	if !evt.Redacted {
		t.Error("Event should be redacted by default")
	}
}

func TestCaseBundle(t *testing.T) {
	m := NewManifest("case-001")
	bundle := CaseBundle{
		Manifest: m,
		Events: []Event{
			{ID: "evt_001", EventType: "tool_call", Decision: "allow", Redacted: true, Timestamp: "2026-07-30T12:00:00Z"},
		},
		Decisions: nil,
	}
	if bundle.Manifest.CaseID != "case-001" {
		t.Errorf("Bundle CaseID = %q, want case-001", bundle.Manifest.CaseID)
	}
	if len(bundle.Events) != 1 {
		t.Errorf("Expected 1 event, got %d", len(bundle.Events))
	}
}

func TestProposalAppliedRecord(t *testing.T) {
	rec := ProposalApplied{
		ProposalID: "prop_001",
		Action:     "set",
		Rule:       `{"match":{"server":"symseek"},"decision":"allow"}`,
		AppliedBy:  "human-1",
		AppliedAt:  "2026-08-06T12:00:00Z",
	}
	if rec.ProposalID != "prop_001" || rec.Action != "set" || rec.AppliedBy != "human-1" {
		t.Errorf("ProposalApplied fields mismatch: %+v", rec)
	}
	data, err := json.Marshal(rec)
	if err != nil {
		t.Fatalf("Marshal: %v", err)
	}
	var got map[string]any
	if err := json.Unmarshal(data, &got); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}
	for _, key := range []string{"proposal_id", "action", "rule", "applied_by", "applied_at"} {
		if _, ok := got[key]; !ok {
			t.Errorf("record missing json key %q", key)
		}
	}
}
