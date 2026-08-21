package sequence

import (
	"sort"

	"github.com/danieljustus/symaira-guard/internal/model"
)

// Ledger is a structured record separating successful from failed calls per
// tool, so a downstream summarizer can surface failures explicitly (job 3 of
// the sequence detector).
//
// Outcome derivation: ActionCompleted without Call.Error is a success;
// ActionCompleted with Call.Error and ActionFailed are failures; all other
// states are ignored. The ledger is an unbounded summary counter, not a
// window — bounded memory lives in the Detector window, not here.
type Ledger struct {
	entries map[string]*LedgerEntry
}

// LedgerEntry aggregates outcomes for one tool.
type LedgerEntry struct {
	Tool      string
	Success   int
	Failed    int
	LastError string // most recent failure message, if any
}

// NewLedger returns an empty ledger.
func NewLedger() *Ledger {
	return &Ledger{entries: make(map[string]*LedgerEntry)}
}

// Record folds one event's outcome into the ledger. Events without a
// terminal outcome are ignored.
func (l *Ledger) Record(ev model.ActionEvent) {
	outcome, ok := outcomeOf(ev)
	if !ok {
		return
	}
	tool := ev.Call.Tool
	e := l.entries[tool]
	if e == nil {
		e = &LedgerEntry{Tool: tool}
		l.entries[tool] = e
	}
	switch outcome {
	case outcomeSuccess:
		e.Success++
	case outcomeFailed:
		e.Failed++
		if ev.Call.Error != "" {
			e.LastError = ev.Call.Error
		}
	}
}

// Summarize returns per-tool outcome counts, sorted by tool name.
func (l *Ledger) Summarize() []LedgerEntry {
	out := make([]LedgerEntry, 0, len(l.entries))
	for _, e := range l.entries {
		out = append(out, *e)
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Tool < out[j].Tool })
	return out
}

// outcome classifies a terminal event state.
type outcome int

const (
	outcomeNone outcome = iota
	outcomeSuccess
	outcomeFailed
)

// outcomeOf maps an event to its ledger outcome.
func outcomeOf(ev model.ActionEvent) (outcome, bool) {
	switch ev.State {
	case model.ActionCompleted:
		if ev.Call.Error != "" {
			return outcomeFailed, true
		}
		return outcomeSuccess, true
	case model.ActionFailed:
		return outcomeFailed, true
	}
	return outcomeNone, false
}
