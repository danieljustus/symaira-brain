package audit

import (
	"bytes"
	"encoding/json"
	"os"
	"testing"
	"time"
)

func TestLog_RecordsFailureClassification(t *testing.T) {
	f := newTestFile(t)
	l := &Logger{
		f:       f,
		path:    f.Name(),
		profile: "test",
		config:  Config{Enabled: true},
	}

	l.Log("memory", "memory_search", nil, 12*time.Millisecond, "error", Classification{Category: "timeout", Retryable: true})
	l.LogDegradation("vault", "child unavailable", "warning")
	data, err := os.ReadFile(f.Name())
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}
	var entry Entry
	lines := bytes.Split(bytes.TrimSpace(data), []byte{'\n'})
	if err := json.Unmarshal(lines[0], &entry); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}
	if entry.Category != "timeout" || !entry.Retryable {
		t.Fatalf("classification = {%q %v}, want {timeout true}", entry.Category, entry.Retryable)
	}
}
