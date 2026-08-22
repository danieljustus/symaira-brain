package audit

import (
	"encoding/json"
	"testing"
	"time"
)

func TestLog_RecordsFailureClassification(t *testing.T) {
	l := newTestLogger(t)

	l.Log("memory", "memory_search", nil, 12*time.Millisecond, "error", Classification{Category: "timeout", Retryable: true})
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
