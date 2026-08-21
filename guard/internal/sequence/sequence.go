// Package sequence implements a stateful, bounded-window detector for
// repetitive tool-call patterns in the model.ActionEvent stream — the
// sequence-aware counterpart to the per-call rules in internal/policy.
//
// # Why this exists
//
// A per-call rule evaluates one event in isolation: an agent that calls the
// same tool with the same input twenty times, or issues twenty near-identical
// searches, passes every individual check while burning budget and going
// nowhere. This detector runs a sliding window over the call stream and
// blocks repetition that no single-call rule can see. It is a pattern
// adoption of pssah4/vault-operator's ToolRepetitionDetector (see
// docs/adopt/2026-08-06T14-20-07Z--pssah4-vault-operator.md).
//
// # Three jobs
//
//  1. Exact repetition: a bounded window keyed by (tool, canonicalized-input
//     hash) blocks the Nth in-window call with the same key (default N=3,
//     configurable via Config.Threshold). Input canonicalization is JSON
//     serialization — encoding/json sorts map keys, so semantically equal
//     maps hash identically; serialization failures fall back to fmt %v.
//  2. Fuzzy dedup of search-shaped tools: a tool whose lowercased name
//     contains "search" or "find" (e.g. search_web, find_files) is treated as
//     search-shaped. Its input is tokenized (lowercase; split on
//     non-alphanumeric runes; tokens shorter than 2 runes dropped), and a new
//     query whose Jaccard term overlap (|A∩B|/|A∪B|) with an in-window query
//     of the same tool reaches Config.FuzzyRatio (default 0.8) counts against
//     that query's repetition count.
//  3. Outcome ledger: completed/failed events are recorded per tool so a
//     downstream summarizer can surface failures explicitly (see Ledger).
//
// # Recoverability contract
//
// A deny from this detector is ALWAYS recoverable: Evaluation.Recoverable is
// true whenever Evaluation.Decision is model.DecisionDeny, and the detector
// never emits a terminal/abort signal. It refuses the individual call; the
// agent may proceed with a different call immediately. Every
// Evaluation.Reason starts with ReasonPrefix ("sequence: ") so consumers can
// identify detector output by prefix.
//
// # Event feed contract
//
// Each logical call attempt must be fed exactly once as ActionRequested or
// ActionStarted — the only states that count toward the window. Feeding the
// full requested→approved→started→completed lifecycle would multiply the
// count per logical call. Outcome states are folded into the ledger:
// ActionCompleted without Call.Error is a success; ActionCompleted with
// Call.Error and ActionFailed are failures; all other states are ignored.
//
// # Bounded memory
//
// The window keeps at most Config.WindowSize distinct signatures (default
// 100). When a new signature would exceed the cap, the least-recently-used
// signature is evicted and its count forgotten, so memory stays flat
// regardless of stream length.
//
// # Opt-in
//
// The detector is inert unless Config.Enabled is set (the TOML [sequence]
// section, off by default) — symguard stays stateless by default. A disabled
// detector always allows and records nothing.
package sequence

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"strings"
	"unicode"

	"github.com/danieljustus/symaira-guard/internal/model"
)

// Defaults for Config fields when left zero.
const (
	DefaultThreshold  = 3
	DefaultWindowSize = 100
	DefaultFuzzyRatio = 0.8
)

// ReasonPrefix prefixes every Evaluation.Reason produced by the detector.
const ReasonPrefix = "sequence: "

// Config controls the sequence detector.
type Config struct {
	// Enabled turns the detector on. When false, Evaluate always allows
	// and records nothing.
	Enabled bool
	// Threshold is the in-window call count that blocks a signature:
	// a call whose key already has count >= Threshold is denied.
	// Zero or negative means DefaultThreshold.
	Threshold int
	// WindowSize caps the number of distinct signatures tracked. Zero or
	// negative means DefaultWindowSize.
	WindowSize int
	// FuzzyRatio is the minimum Jaccard term overlap (0..1) between two
	// search-shaped queries for them to count as duplicates. Zero or
	// negative means DefaultFuzzyRatio; values above 1 clamp to 1.
	FuzzyRatio float64
}

// Evaluation is the structured outcome of checking one event against the
// window.
//
// Contract: model.DecisionDeny always comes with Recoverable=true and a
// Reason prefixed with ReasonPrefix. The detector never aborts a session;
// it only refuses the individual call.
type Evaluation struct {
	Decision    model.Decision
	Reason      string
	Recoverable bool
	Key         string // matched window key, for diagnostics
	Count       int    // in-window occurrences of the key after this call
}

// Detector is a stateful, bounded-window repetition detector.
type Detector struct {
	cfg     Config
	seq     int64
	entries map[string]*entry
	ledger  *Ledger
}

// entry is one in-window signature. Search-shaped entries keep their
// tokenized terms so later queries can be compared for overlap.
type entry struct {
	key     string
	tool    string
	search  bool
	terms   []string
	count   int
	lastSeq int64
}

// NewDetector creates a detector, applying defaults to zero Config fields.
func NewDetector(cfg Config) *Detector {
	if cfg.Threshold <= 0 {
		cfg.Threshold = DefaultThreshold
	}
	if cfg.WindowSize <= 0 {
		cfg.WindowSize = DefaultWindowSize
	}
	if cfg.FuzzyRatio <= 0 {
		cfg.FuzzyRatio = DefaultFuzzyRatio
	}
	if cfg.FuzzyRatio > 1 {
		cfg.FuzzyRatio = 1
	}
	return &Detector{
		cfg:     cfg,
		entries: make(map[string]*entry),
		ledger:  NewLedger(),
	}
}

// Evaluate checks one event against the window and ledger, returning the
// resulting decision. See the package doc for the event-feed contract and
// the recoverability contract.
func (d *Detector) Evaluate(ev model.ActionEvent) Evaluation {
	if !d.cfg.Enabled {
		return Evaluation{Decision: model.DecisionAllow, Reason: ReasonPrefix + "disabled"}
	}

	d.ledger.Record(ev)

	if ev.State != model.ActionRequested && ev.State != model.ActionStarted {
		return Evaluation{
			Decision: model.DecisionAllow,
			Reason:   ReasonPrefix + "not an attempt (state " + string(ev.State) + ")",
		}
	}

	tool := ev.Call.Tool
	search := isSearchTool(tool)
	key, terms := d.keyFor(tool, ev.Call.Args, search)

	d.seq++
	if e := d.entries[key]; e != nil {
		e.count++
		e.lastSeq = d.seq
		if e.count >= d.cfg.Threshold {
			return Evaluation{
				Decision:    model.DecisionDeny,
				Reason:      denyReason(tool, e),
				Recoverable: true,
				Key:         key,
				Count:       e.count,
			}
		}
		return Evaluation{
			Decision: model.DecisionAllow,
			Reason:   ReasonPrefix + "no repetition detected",
			Key:      key,
			Count:    e.count,
		}
	}

	d.entries[key] = &entry{
		key:     key,
		tool:    tool,
		search:  search,
		terms:   terms,
		count:   1,
		lastSeq: d.seq,
	}
	d.evict()
	return Evaluation{
		Decision: model.DecisionAllow,
		Reason:   ReasonPrefix + "no repetition detected",
		Key:      key,
		Count:    1,
	}
}

// Ledger returns the outcome ledger maintained by the detector.
func (d *Detector) Ledger() *Ledger { return d.ledger }

// keyFor resolves the window key for a call: an exact (tool, input-hash) key
// for ordinary tools, or a fuzzy-resolved search key for search-shaped tools.
func (d *Detector) keyFor(tool string, args any, search bool) (string, []string) {
	if !search {
		return exactKey(tool, canonicalizeInput(args)), nil
	}
	terms := tokenize(canonicalizeInput(args))
	return d.fuzzyKey(tool, terms), terms
}

// fuzzyKey returns the key of the in-window search query that overlaps the
// new query by at least FuzzyRatio, preferring the strongest overlap (ties
// go to the higher count). Without a match it mints a fresh search key.
func (d *Detector) fuzzyKey(tool string, terms []string) string {
	if len(terms) == 0 {
		return searchKey(tool, terms)
	}
	var best *entry
	bestOverlap := 0.0
	for _, e := range d.entries {
		if !e.search || e.tool != tool || len(e.terms) == 0 {
			continue
		}
		o := termOverlap(e.terms, terms)
		if o >= d.cfg.FuzzyRatio &&
			(best == nil || o > bestOverlap || (o == bestOverlap && e.count > best.count)) {
			best = e
			bestOverlap = o
		}
	}
	if best != nil {
		return best.key
	}
	return searchKey(tool, terms)
}

// evict drops the least-recently-used signature while the window exceeds
// WindowSize entries. The just-inserted entry has the highest sequence
// number, so it is never the victim.
func (d *Detector) evict() {
	for len(d.entries) > d.cfg.WindowSize {
		var victim *entry
		for _, e := range d.entries {
			if victim == nil || e.lastSeq < victim.lastSeq {
				victim = e
			}
		}
		if victim == nil {
			return
		}
		delete(d.entries, victim.key)
	}
}

// denyReason describes the blocked repetition for the reason string.
func denyReason(tool string, e *entry) string {
	kind := "exact repetition"
	if e.search {
		kind = "fuzzy search duplicate"
	}
	return fmt.Sprintf("%s%s of tool %q (%d calls in window)", ReasonPrefix, kind, tool, e.count)
}

// exactKey hashes the canonicalized input and keys it under the tool name.
func exactKey(tool, input string) string {
	sum := sha256.Sum256([]byte(input))
	return "exact:" + tool + "\x00" + hex.EncodeToString(sum[:])
}

// searchKey keys a search query under its sorted token terms.
func searchKey(tool string, terms []string) string {
	return "search:" + tool + "\x00" + strings.Join(terms, "\x00")
}

// canonicalizeInput renders args deterministically: JSON with map keys
// sorted (encoding/json sorts map keys), falling back to fmt %v when
// serialization fails.
func canonicalizeInput(args any) string {
	if args == nil {
		return ""
	}
	b, err := json.Marshal(args)
	if err != nil {
		return fmt.Sprintf("%v", args)
	}
	return string(b)
}

// tokenize splits search input into lowercase terms, dropping tokens
// shorter than 2 runes.
func tokenize(s string) []string {
	fields := strings.FieldsFunc(strings.ToLower(s), func(r rune) bool {
		return !unicode.IsLetter(r) && !unicode.IsDigit(r)
	})
	out := make([]string, 0, len(fields))
	for _, f := range fields {
		if len(f) >= 2 {
			out = append(out, f)
		}
	}
	return out
}

// termOverlap is the Jaccard overlap |A∩B|/|A∪B| of two term sets. Two empty
// term sets count as identical (overlap 1) so an empty query still
// deduplicates against itself via the exact search key.
func termOverlap(a, b []string) float64 {
	setA := termSet(a)
	setB := termSet(b)
	if len(setA) == 0 && len(setB) == 0 {
		return 1
	}
	inter := 0
	for t := range setA {
		if setB[t] {
			inter++
		}
	}
	union := len(setA) + len(setB) - inter
	if union == 0 {
		return 1
	}
	return float64(inter) / float64(union)
}

// termSet dedupes a token slice into a set.
func termSet(terms []string) map[string]bool {
	set := make(map[string]bool, len(terms))
	for _, t := range terms {
		set[t] = true
	}
	return set
}

// isSearchTool reports whether the tool name marks it as search-shaped:
// its lowercased name contains "search" or "find".
func isSearchTool(name string) bool {
	n := strings.ToLower(name)
	return strings.Contains(n, "search") || strings.Contains(n, "find")
}
