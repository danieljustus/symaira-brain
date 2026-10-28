//go:build unix

package usage

import (
	"context"
	"fmt"
	"testing"
	"time"
)

func TestGrokHistoryDSTCalendar(t *testing.T) {
	zone, err := time.LoadLocation("Europe/Berlin")
	if err != nil {
		t.Fatal(err)
	}
	now, err := time.Parse(time.RFC3339, "2026-10-26T00:30:00+01:00")
	if err != nil {
		t.Fatal(err)
	}
	now = now.In(zone)
	var data string
	for i, at := range []string{"2026-10-24T21:59:59Z", "2026-10-24T22:30:00Z", "2026-10-25T22:30:00Z", "2026-10-25T23:10:00Z"} {
		data += fmt.Sprintf("{\"timestamp\":%q,\"params\":{\"_meta\":{\"eventId\":\"synthetic-%d\"},\"update\":{\"sessionUpdate\":\"turn_completed\",\"usage\":{\"modelUsage\":{\"grok-build\":{\"inputTokens\":1,\"outputTokens\":0,\"costUsdTicks\":0}}}}}}\n", at, i)
	}
	h := ScanGrokHistory(context.Background(), historyFixture(t, map[string]string{"p/updates.jsonl": data}), now)
	for _, b := range h.Periods {
		switch b.Key {
		case "today":
			if b.Models[0].Events != 1 {
				t.Fatal(b)
			}
		case "yesterday":
			if b.Models[0].Events != 2 {
				t.Fatal(b)
			}
		}
	}
	if len(h.Daily) != 3 || len(h.Periods) != 3 || h.Status != "available" {
		t.Fatal(h)
	}
}
