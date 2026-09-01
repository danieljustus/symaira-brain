package mcp

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/activity"
	"github.com/danieljustus/symaira-corekit/mcpserver"
)

func TestActivityGetAndStatusHandlers(t *testing.T) {
	s := helperServer(t)
	// Anchored to now, not to a fixed calendar date: the store stamps
	// ExpiresAt = EndedAt + SegmentTTL (48h by default) and Search filters
	// on expires_at > now, so a hardcoded date makes the test pass only
	// until 48h after it — this one went red mid-day on 2026-09-01 and
	// stayed red. activity_api_test.go already anchors the same way.
	base := time.Now().UTC().Add(-time.Hour).Truncate(time.Second)
	if err := s.activityStore.SaveSegment(activity.Segment{
		Source:          "editor",
		Granularity:     activity.Granularity10Min,
		StartedAt:       base,
		EndedAt:         base.Add(10 * time.Minute),
		RedactedSummary: "bounded segment contact alice@example.com",
		RawRef:          "capture-ref",
	}); err != nil {
		t.Fatal(err)
	}
	page, err := s.activityStore.Search(activity.SearchOptions{
		Query: "bounded", From: base.Add(-time.Minute), To: base.Add(time.Hour), Limit: 1, MaxTokens: 200,
	})
	if err != nil {
		t.Fatal(err)
	}
	if len(page.Results) != 1 {
		t.Fatalf("search results = %+v", page.Results)
	}
	id := page.Results[0].ID

	t.Run("get returns fenced redacted item", func(t *testing.T) {
		input := json.RawMessage(fmt.Sprintf(`{"id":%q,"max_tokens":200}`, id))
		value, err := s.handleActivityGet(context.Background(), input)
		if err != nil {
			t.Fatal(err)
		}
		text, ok := value.(string)
		if !ok {
			t.Fatalf("response type = %T", value)
		}
		if !strings.Contains(text, activityUntrustedFenceStart) || !strings.Contains(text, id) {
			t.Fatalf("response missing bounded item: %s", text)
		}
		if strings.Contains(text, "alice@example.com") {
			t.Fatalf("response leaked raw PII: %s", text)
		}
	})

	t.Run("get returns not found text", func(t *testing.T) {
		value, err := s.handleActivityGet(context.Background(), json.RawMessage(`{"id":"missing","max_tokens":100}`))
		if err != nil {
			t.Fatal(err)
		}
		if value != "activity not found: missing" {
			t.Fatalf("value = %#v", value)
		}
	})

	getCases := []struct {
		name  string
		input string
		want  string
	}{
		{"malformed JSON", `{`, "failed to parse arguments"},
		{"missing fields", `{}`, "id and max_tokens are required"},
		{"blank id", `{"id":" ","max_tokens":100}`, "id and max_tokens are required"},
		{"long id", fmt.Sprintf(`{"id":%q,"max_tokens":100}`, strings.Repeat("x", activity.MaxQueryLength+1)), "id exceeds"},
		{"small token budget", fmt.Sprintf(`{"id":%q,"max_tokens":0}`, id), "max_tokens must be between"},
	}
	for _, tc := range getCases {
		t.Run(tc.name, func(t *testing.T) {
			_, err := s.handleActivityGet(context.Background(), json.RawMessage(tc.input))
			if err == nil || !strings.Contains(err.Error(), tc.want) {
				t.Fatalf("error = %v, want %q", err, tc.want)
			}
		})
	}

	t.Run("get reports unavailable store", func(t *testing.T) {
		withoutStore := helperServer(t)
		withoutStore.activityStore = nil
		_, err := withoutStore.handleActivityGet(context.Background(), json.RawMessage(`{"id":"id","max_tokens":100}`))
		if err == nil || !strings.Contains(err.Error(), "store unavailable") {
			t.Fatalf("error = %v", err)
		}
	})

	t.Run("get reports closed store", func(t *testing.T) {
		closed := helperServer(t)
		if err := closed.activityStore.Database().Close(); err != nil {
			t.Fatal(err)
		}
		_, err := closed.handleActivityGet(context.Background(), json.RawMessage(`{"id":"id","max_tokens":100}`))
		if err == nil || !strings.Contains(err.Error(), "activity_get") {
			t.Fatalf("error = %v", err)
		}
	})

	t.Run("status returns counts without content", func(t *testing.T) {
		value, err := s.handleActivityStatus(context.Background(), json.RawMessage(`{"max_tokens":100}`))
		if err != nil {
			t.Fatal(err)
		}
		text, ok := value.(string)
		if !ok {
			t.Fatalf("response type = %T", value)
		}
		var status activity.Status
		if err := json.Unmarshal([]byte(text), &status); err != nil {
			t.Fatalf("decode status: %v (%s)", err, text)
		}
		if status.ActiveSegments != 1 || status.ActiveEpisodes != 0 {
			t.Fatalf("status = %+v", status)
		}
		if strings.Contains(text, "bounded segment") || strings.Contains(text, "alice@example.com") {
			t.Fatalf("status returned activity content: %s", text)
		}
	})

	statusCases := []struct {
		name  string
		input string
		want  string
	}{
		{"malformed JSON", `{`, "failed to parse arguments"},
		{"missing budget", `{}`, "max_tokens is required"},
		{"zero budget", `{"max_tokens":0}`, "max_tokens must be between"},
		{"large budget", `{"max_tokens":4001}`, "max_tokens must be between"},
	}
	for _, tc := range statusCases {
		t.Run(tc.name, func(t *testing.T) {
			_, err := s.handleActivityStatus(context.Background(), json.RawMessage(tc.input))
			if err == nil || !strings.Contains(err.Error(), tc.want) {
				t.Fatalf("error = %v, want %q", err, tc.want)
			}
		})
	}

	t.Run("status reports unavailable store", func(t *testing.T) {
		withoutStore := helperServer(t)
		withoutStore.activityStore = nil
		_, err := withoutStore.handleActivityStatus(context.Background(), json.RawMessage(`{"max_tokens":100}`))
		if err == nil || !strings.Contains(err.Error(), "store unavailable") {
			t.Fatalf("error = %v", err)
		}
	})

	t.Run("status reports closed store", func(t *testing.T) {
		closed := helperServer(t)
		if err := closed.activityStore.Database().Close(); err != nil {
			t.Fatal(err)
		}
		_, err := closed.handleActivityStatus(context.Background(), json.RawMessage(`{"max_tokens":100}`))
		if err == nil || !strings.Contains(err.Error(), "activity_status") {
			t.Fatalf("error = %v", err)
		}
	})
}

func TestRegisterActivityToolsForAllowedProfile(t *testing.T) {
	s := helperServer(t)
	srv := mcpserver.New("test", "test")
	s.RegisterTools(srv, map[string]bool{
		"activity_search": true,
		"activity_get":    true,
		"activity_status": true,
	})
	data, err := json.Marshal(map[string]interface{}{
		"jsonrpc": "2.0",
		"id":      "test-1",
		"method":  "tools/list",
	})
	if err != nil {
		t.Fatal(err)
	}
	var input, output bytes.Buffer
	input.Write(frameRequest(data))
	if err := srv.ServeIO(context.Background(), &input, &output); err != nil {
		t.Fatal(err)
	}
	response := readFramedResponse(&output)
	result, ok := response["result"].(map[string]interface{})
	if !ok {
		t.Fatalf("tools/list response = %#v", response)
	}
	tools, ok := result["tools"].([]interface{})
	if !ok {
		t.Fatalf("tools = %#v", result["tools"])
	}
	found := map[string]bool{}
	for _, raw := range tools {
		tool, ok := raw.(map[string]interface{})
		if ok {
			if name, ok := tool["name"].(string); ok {
				found[name] = true
			}
		}
	}
	for _, name := range []string{"activity_search", "activity_get", "activity_status"} {
		if !found[name] {
			t.Errorf("allowed activity tool %q missing from tools/list", name)
		}
	}
}

func TestActivityFenceSummaryBoundaries(t *testing.T) {
	full := fenceActivitySummary("short summary", 400)
	if !strings.HasPrefix(full, activityUntrustedFenceStart+"\n") || !strings.HasSuffix(full, "\n"+activityUntrustedFenceEnd) {
		t.Fatalf("full fence = %q", full)
	}
	if !strings.Contains(full, "short summary") {
		t.Fatalf("full fence lost summary: %q", full)
	}

	minimal := fenceActivitySummary("summary", 1)
	if minimal != activityUntrustedFenceStart+"\n"+activityUntrustedFenceEnd {
		t.Fatalf("minimal fence = %q", minimal)
	}

	truncated := fenceActivitySummary(strings.Repeat("long summary ", 100), 20)
	if !strings.HasPrefix(truncated, activityUntrustedFenceStart+"\n") || !strings.HasSuffix(truncated, "\n"+activityUntrustedFenceEnd) {
		t.Fatalf("truncated fence = %q", truncated)
	}
	if len([]rune(truncated)) >= len([]rune(fenceActivitySummary(strings.Repeat("long summary ", 100), 400))) {
		t.Fatalf("summary was not truncated: %d runes", len([]rune(truncated)))
	}
	if activityTokenCount("") != 0 || activityTokenCount("x") != 1 {
		t.Fatalf("unexpected token counts: empty=%d one=%d", activityTokenCount(""), activityTokenCount("x"))
	}
}

func TestMarshalActivityRejectsUnsupportedValue(t *testing.T) {
	if _, err := marshalActivity(func() {}); err == nil || !strings.Contains(err.Error(), "encode activity response") {
		t.Fatalf("marshal error = %v", err)
	}
}
