package mcp

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-browse/internal/daemon"
)

func TestSEC007MCPErrorCorpusRedactsEveryPublishedField(t *testing.T) {
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
	response := daemon.Response{Error: &daemon.Error{
		Code:       daemon.ErrorOperationFailed,
		Message:    corpus.Text,
		Hint:       corpus.Text,
		ResumeHint: corpus.Text,
		Details:    corpus.JSON,
	}}
	err = daemonToolError(response)
	metadata, ok := err.(interface {
		Error() string
		ErrorHint() string
		ResumeGuidance() string
		ErrorDetails() map[string]any
	})
	if !ok {
		t.Fatalf("MCP error type %T does not expose structured metadata", err)
	}
	details, marshalErr := json.Marshal(metadata.ErrorDetails())
	if marshalErr != nil {
		t.Fatal(marshalErr)
	}
	combined := metadata.Error() + metadata.ErrorHint() + metadata.ResumeGuidance() + string(details)
	if !strings.Contains(combined, "status: 401") || !strings.Contains(combined, daemon.ErrorOperationFailed) {
		t.Fatalf("MCP redaction lost useful error context: %s", combined)
	}
	for _, marker := range corpus.SecretValues {
		if strings.Contains(combined, marker) {
			t.Fatalf("MCP error metadata leaked synthetic marker %q", marker)
		}
	}
}
