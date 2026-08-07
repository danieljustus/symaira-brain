package gateway

import (
	"encoding/json"
	"errors"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/audit"
	"github.com/danieljustus/symaira-brain/internal/broker"
)

func TestClassifyError_CoversBrokerFailureTypes(t *testing.T) {
	tests := []struct {
		name      string
		err       error
		category  string
		retryable bool
	}{
		{name: "rpc", err: &broker.RPCError{Code: -32602, Message: "invalid params"}, category: "rpc"},
		{name: "timeout", err: &broker.TimeoutError{Op: "tools/call"}, category: "timeout", retryable: true},
		{name: "closed", err: &broker.ClosedError{Op: "tools/call", Err: errors.New("exit status 1")}, category: "closed", retryable: true},
		{name: "tool", err: &classifiedError{message: "tool error: entry not found", Classification: audit.Classification{Category: "tool", Retryable: false}}, category: "tool"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := classifyError(tt.err)
			if got.Category != tt.category || got.Retryable != tt.retryable {
				t.Fatalf("classification = %+v, want category=%q retryable=%v", got, tt.category, tt.retryable)
			}
		})
	}
}

func TestClassifiedError_ErrorCarriesUnchangedMessageAndClassification(t *testing.T) {
	const message = "tool error: entry not found"
	err := &classifiedError{message: message, Classification: audit.Classification{Category: "tool", Retryable: false}}

	var payload struct {
		Message   string `json:"message"`
		Category  string `json:"category"`
		Retryable bool   `json:"retryable"`
	}
	if err := json.Unmarshal([]byte(err.Error()), &payload); err != nil {
		t.Fatalf("classified error is not JSON: %v", err)
	}
	if payload.Message != message {
		t.Errorf("message = %q, want %q", payload.Message, message)
	}
	if payload.Category != "tool" || payload.Retryable {
		t.Errorf("classification = {%q %v}, want {tool false}", payload.Category, payload.Retryable)
	}
}
