package daemon

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

type secretCorpus struct {
	SecretValues []string       `json:"secret_values"`
	Text         string         `json:"text"`
	JSON         map[string]any `json:"json"`
}

func loadSecretCorpus(t *testing.T) secretCorpus {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("..", "..", "testdata", "port", "security", "redaction-corpus.json"))
	if err != nil {
		t.Fatal(err)
	}
	var corpus secretCorpus
	if err := json.Unmarshal(data, &corpus); err != nil {
		t.Fatal(err)
	}
	return corpus
}

func TestSEC007DiagnosticCorpusPreservesUsefulErrorContext(t *testing.T) {
	corpus := loadSecretCorpus(t)
	text := RedactDiagnostic(corpus.Text)
	structured, err := json.Marshal(RedactDiagnosticValue(corpus.JSON))
	if err != nil {
		t.Fatal(err)
	}
	combined := text + string(structured)
	if !strings.Contains(combined, "status: 401") {
		t.Fatalf("redaction removed useful error status: %s", combined)
	}
	for _, marker := range corpus.SecretValues {
		if strings.Contains(combined, marker) {
			t.Fatalf("diagnostic output leaked synthetic marker %q", marker)
		}
	}
	response := ErrorResponse(ErrorOperationFailed, corpus.Text)
	encoded, err := json.Marshal(response)
	if err != nil {
		t.Fatal(err)
	}
	for _, marker := range corpus.SecretValues {
		if strings.Contains(string(encoded), marker) {
			t.Fatalf("daemon error response leaked synthetic marker %q", marker)
		}
	}
}
