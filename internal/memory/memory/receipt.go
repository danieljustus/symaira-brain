package memory

import (
	"fmt"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

// receiptContentPrefix caps how much content a receipt quotes. Receipts are
// one-liners meant to be echoed verbatim by an agent, so they must stay
// short and deterministic.
const receiptContentPrefix = 60

// Receipt mints a deterministic one-line recall receipt for a stored
// memory, e.g.:
//
//	◉ memory: "daemon runs on port 8787" (project, 3d)
//
// The engine — not the agent — mints the string, so an agent cannot
// fabricate a citation for a memory that was never returned. The receipt
// references the memory by its content prefix, scope and age; pairing it
// with the query-log result rows (issue #460) makes a claimed receipt
// checkable after the fact. now is injectable so callers can keep output
// deterministic.
func Receipt(m *db.Memory, now time.Time) string {
	if m == nil {
		return ""
	}
	prefix := strings.Join(strings.Fields(m.Content), " ")
	prefix = strings.ReplaceAll(prefix, `"`, `'`)
	if len(prefix) > receiptContentPrefix {
		prefix = prefix[:receiptContentPrefix]
	}
	return fmt.Sprintf("◉ memory: %q (%s, %s)", prefix, m.Scope, relativeAge(m.CreatedAt, now))
}

// relativeAge renders the memory's age relative to now in a compact,
// deterministic form: "just now", "5m", "3h", "2d", "45d".
func relativeAge(t, now time.Time) string {
	if t.IsZero() {
		return "unknown"
	}
	d := now.Sub(t)
	switch {
	case d < time.Minute:
		return "just now"
	case d < time.Hour:
		return fmt.Sprintf("%dm", int(d.Minutes()))
	case d < 24*time.Hour:
		return fmt.Sprintf("%dh", int(d.Hours()))
	default:
		return fmt.Sprintf("%dd", int(d.Hours()/24))
	}
}
