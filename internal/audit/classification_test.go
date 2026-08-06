package audit

import (
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
	data, err := os.ReadFile(f.Name())
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}
	var entry Entry
	if err := json.Unmarshal(data, &entry); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}
	if entry.Category != "timeout" || !entry.Retryable {
		t.Fatalf("classification = {%q %v}, want {timeout true}", entry.Category, entry.Retryable)
	}
}
