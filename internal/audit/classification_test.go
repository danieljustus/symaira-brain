package audit

import (
	"encoding/json"
	"testing"
	"time"
)

func TestLog_RecordsFailureClassification(t *testing.T) {
	l := newTestLogger(t)

	l.Log("memory", "memory_search", nil, 12*time.Millisecond, "error", Exposure{}, Classification{Category: "timeout", Retryable: true})
	l.LogDegradation("vault", "child unavailable", "warning")
	payloads := readPayloads(t, l.path)
	var entry Entry
	if err := json.Unmarshal([]byte(payloads[0]), &entry); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}
	if entry.Category != "timeout" || !entry.Retryable {
		t.Fatalf("classification = {%q %v}, want {timeout true}", entry.Category, entry.Retryable)
	}
}

func TestLog_RecordsForeignExposure(t *testing.T) {
	// A foreign-server tool call records its access class and derivation in
	// the audit entry so the exposure decision is explainable (issue #335).
	l := newTestLogger(t)

	l.Log("fig", "search", json.RawMessage(`{"q":"x"}`), 5*time.Millisecond, "ok", Exposure{
		AccessClass:  "read",
		AccessSource: "read_only_hint",
	})
	payloads := readPayloads(t, l.path)
	var entry Entry
	if err := json.Unmarshal([]byte(payloads[0]), &entry); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}
	if entry.AccessClass != "read" || entry.AccessSource != "read_only_hint" {
		t.Errorf("exposure = %q/%q, want read/read_only_hint", entry.AccessClass, entry.AccessSource)
	}

	// Core servers leave the exposure fields empty (omitted from the JSON
	// via omitempty — a fresh struct is required because json.Unmarshal
	// does not zero absent fields).
	l.Log("vault", "get_entry", nil, time.Millisecond, "ok", Exposure{})
	payloads = readPayloads(t, l.path)
	var coreEntry Entry
	if err := json.Unmarshal([]byte(payloads[1]), &coreEntry); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}
	if coreEntry.AccessClass != "" || coreEntry.AccessSource != "" {
		t.Errorf("core exposure = %q/%q, want empty", coreEntry.AccessClass, coreEntry.AccessSource)
	}
}
