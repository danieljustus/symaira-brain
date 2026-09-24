package journal

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestSEC007SyntheticCorpusDoesNotEnterJournalFiles(t *testing.T) {
	data, err := os.ReadFile(filepath.Join("..", "..", "testdata", "port", "security", "redaction-corpus.json"))
	if err != nil {
		t.Fatal(err)
	}
	var corpus struct {
		SecretValues []string       `json:"secret_values"`
		Text         string         `json:"text"`
		JSON         map[string]any `json:"json"`
	}
	if err := json.Unmarshal(data, &corpus); err != nil {
		t.Fatal(err)
	}
	redactor := DefaultRedactor()
	redactor.Values = append(redactor.Values, corpus.SecretValues...)
	journal := newTestJournal(t, "sec007", redactor)
	if _, err := journal.Append(Entry{
		Command: "redaction-corpus",
		Args:    corpus.JSON,
		Reason:  corpus.Text,
	}); err != nil {
		t.Fatal(err)
	}
	output, err := os.ReadFile(journal.Path())
	if err != nil {
		t.Fatal(err)
	}
	for _, marker := range corpus.SecretValues {
		if strings.Contains(string(output), marker) {
			t.Fatalf("journal file leaked synthetic marker %q", marker)
		}
	}
	if !strings.Contains(string(output), "status: 401") {
		t.Fatalf("journal lost useful error context: %s", output)
	}
}
