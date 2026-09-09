package gateway

import (
	"context"
	"encoding/json"
	"testing"
)

func TestUsageToolHandlerIsAuditWrapped(t *testing.T) {
	sink := &recordingAuditSink{}
	s := &Server{}
	handler := s.wrapInProcess(sink, nil, "usage")("get_ai_usage", func(context.Context, json.RawMessage) (any, error) {
		return map[string]any{"schema_version": 1}, nil
	})
	if _, err := handler(context.Background(), json.RawMessage(`{}`)); err != nil {
		t.Fatal(err)
	}
	calls := sink.snapshot()
	if len(calls) != 1 {
		t.Fatalf("audit calls = %d, want 1", len(calls))
	}
	if calls[0].server != "usage" || calls[0].tool != "get_ai_usage" || calls[0].status != "ok" {
		t.Fatalf("audit call = %+v, want usage/get_ai_usage/ok", calls[0])
	}
}
