package usage

import (
	"context"
	"encoding/json"
	"fmt"
	"path/filepath"
	"strings"
	"time"
)

// Cache and reasoning are subsets, not additional tokens.
type codexCounts [4]uint64

func codexUsage(raw json.RawMessage) (*codexCounts, bool) {
	var u map[string]json.RawMessage
	if json.Unmarshal(raw, &u) != nil || u == nil {
		return nil, false
	}
	var b codexCounts
	for i, keys := range [][]string{{"input_tokens", "prompt_tokens", "input"}, {"cached_input_tokens", "cache_read_input_tokens", "cached_tokens"}, {"output_tokens", "completion_tokens", "output"}, {"reasoning_output_tokens", "reasoning_tokens"}} {
		present := false
		for _, key := range keys {
			if v, ok := u[key]; ok {
				n := historyCount(v)
				if n == nil || *n > 1_000_000_000_000 {
					return nil, false
				}
				b[i] = *n
				present = true
				break
			}
		}
		if !present && (i == 0 || i == 2) {
			return nil, false
		}
	}
	if b[1] > b[0] || b[3] > b[2] {
		return nil, false
	}
	if raw, ok := u["total_tokens"]; ok {
		n := historyCount(raw)
		if n == nil || *n != b[0]+b[2] {
			return nil, false
		}
	}
	return &b, true
}

type codexParser struct {
	previous             *codexCounts
	model, tier, session string
	meta, child          bool
	created              *time.Time
	poisoned             bool
	lastOnlySeen         map[string]codexCounts
	replaySeen           map[string]bool
}

func codexModel(p map[string]json.RawMessage) string {
	for _, key := range []string{"model", "model_name"} {
		var s string
		if json.Unmarshal(p[key], &s) == nil && s != "" {
			return s
		}
	}
	var m map[string]json.RawMessage
	if json.Unmarshal(p["metadata"], &m) == nil {
		var s string
		_ = json.Unmarshal(m["model"], &s)
		return s
	}
	return ""
}
func nonNullCodex(v json.RawMessage) bool {
	if len(v) == 0 || string(v) == "null" {
		return false
	}
	var s string
	return json.Unmarshal(v, &s) != nil || strings.TrimSpace(s) != ""
}
func (p *codexParser) parse(line []byte) (*claudeEntry, bool) {
	var o struct {
		Type      string                     `json:"type"`
		Timestamp string                     `json:"timestamp"`
		Payload   map[string]json.RawMessage `json:"payload"`
	}
	if json.Unmarshal(line, &o) != nil || strings.TrimSpace(string(line)) == "null" {
		p.poisoned = true
		return nil, false
	}
	v := o.Payload
	var kind string
	_ = json.Unmarshal(v["type"], &kind)
	if o.Type == "session_meta" && !p.meta {
		p.meta = true
		_ = json.Unmarshal(v["id"], &p.session)
		var source map[string]json.RawMessage
		_ = json.Unmarshal(v["source"], &source)
		p.child = nonNullCodex(v["forked_from_id"]) || nonNullCodex(v["parent_thread_id"]) || string(v["thread_source"]) == `"subagent"` || nonNullCodex(source["subagent"])
		if t, err := time.Parse(time.RFC3339Nano, o.Timestamp); err == nil {
			p.created = &t
		}
		return nil, true
	}
	if o.Type == "turn_context" {
		if m := codexModel(v); m != "" {
			p.model = m
		}
		return nil, true
	}
	if o.Type != "event_msg" {
		return nil, true
	}
	if kind == "thread_settings_applied" {
		var settings map[string]json.RawMessage
		_ = json.Unmarshal(v["thread_settings"], &settings)
		raw, ok := settings["service_tier"]
		if !ok {
			raw = v["service_tier"]
		}
		if len(raw) > 0 {
			if string(raw) == "null" || json.Unmarshal(raw, &p.tier) != nil {
				p.tier = "unsupported"
			}
		}
		return nil, true
	}
	if kind == "task_started" && p.child {
		var n json.Number
		if raw := v["started_at"]; len(raw) > 0 && raw[0] != '"' && json.Unmarshal(raw, &n) == nil {
			start, err := n.Float64()
			gate := p.created
			if gate == nil {
				if t, e := time.Parse(time.RFC3339Nano, o.Timestamp); e == nil {
					gate = &t
				}
			}
			if err == nil && gate != nil && start >= float64(gate.Unix()) {
				p.child = false
			}
		}
		return nil, true
	}
	if kind != "token_count" {
		return nil, true
	}
	var info map[string]json.RawMessage
	if raw := v["info"]; len(raw) == 0 || string(raw) == "null" {
		return nil, true
	}
	if json.Unmarshal(v["info"], &info) != nil {
		p.poisoned = true
		return nil, false
	}
	var totals *codexCounts
	if raw, ok := info["total_token_usage"]; ok && string(raw) != "null" {
		var valid bool
		totals, valid = codexUsage(raw)
		if !valid {
			p.poisoned = true
			return nil, false
		}
	}
	if totals != nil && p.previous != nil && *totals == *p.previous {
		return nil, true
	}
	var used *codexCounts
	if raw, ok := info["last_token_usage"]; ok && string(raw) != "null" {
		var valid bool
		used, valid = codexUsage(raw)
		if !valid {
			p.poisoned = true
			return nil, false
		}
	} else if totals != nil {
		b := *totals
		reset := false
		if p.previous != nil {
			for i, n := range b {
				if n < (*p.previous)[i] {
					reset = true
				}
			}
			if !reset {
				for i := range b {
					b[i] -= (*p.previous)[i]
				}
			}
		}
		used = &b
	}
	if totals != nil {
		p.previous = totals
	}
	if used == nil {
		return nil, true
	}
	at, err := time.Parse(time.RFC3339Nano, o.Timestamp)
	model := codexModel(v)
	if model == "" {
		model = codexModel(info)
	}
	if model != "" {
		p.model = model
	}
	model = p.model
	if model == "" {
		model = "unknown"
	}
	if err != nil || !validHistoryModel(model) {
		p.poisoned = true
		return nil, false
	}
	b := *used
	if b[1] > b[0] || b[3] > b[2] {
		p.poisoned = true
		return nil, false
	}
	total := b[0] + b[2]
	if total == 0 {
		return nil, true
	}
	e := &claudeEntry{event: historyEvent{model: model, at: at, tokens: &total}}
	if p.poisoned {
		if p.child {
			return nil, true
		}
		e.event.tokens = nil
		return e, true
	}
	baselineID := fmt.Sprintf("%s|%s|%s|%v", p.session, at.UTC().Format(time.RFC3339Nano), model, b)
	snapshot := totals
	// Replay and live last-only copies advance the same virtual baseline once,
	// independently of pricing metadata and the chargeable snapshot identity.
	if totals == nil {
		if p.lastOnlySeen == nil {
			p.lastOnlySeen = map[string]codexCounts{}
		}
		next, seen := p.lastOnlySeen[baselineID]
		if !seen {
			next = b
			if p.previous != nil {
				for i := range next {
					next[i] += (*p.previous)[i]
				}
			}
			p.previous = &next
			p.lastOnlySeen[baselineID] = next
		}
		// A missing total uses its original virtual snapshot, even on later copies.
		snapshot = &next
		if p.child {
			if p.replaySeen == nil {
				p.replaySeen = map[string]bool{}
			}
			p.replaySeen[baselineID] = true
		}
		// A copied replay prefix is not made live by a later task-start marker.
		if p.replaySeen[baselineID] {
			return nil, true
		}
	}
	// Tier is hex encoded so its final separator is unambiguous at copy reconciliation.
	e.event.id = fmt.Sprintf("%s|%v|%x", baselineID, *snapshot, p.tier)
	if p.child {
		return nil, true
	}
	return e, true
}

// ScanCodexHistory scans one native home, without credentials, caches or source writes.
func ScanCodexHistory(ctx context.Context, dir string, now time.Time) *LocalHistory {
	h := &LocalHistory{Scope: "local_machine", Attribution: "unattributed", Timezone: now.Location().String(), Status: "no_data", Daily: []HistoryBucket{}, Periods: []HistoryBucket{}}
	if now.Location() == time.Local {
		h.Timezone = "local"
	}
	if !filepath.IsAbs(dir) || strings.ContainsAny(dir, ",\x00") {
		h.Status = "unavailable"
		return h
	}
	for _, part := range strings.Split(dir, "/") {
		if part == "." || part == ".." {
			h.Status = "unavailable"
			return h
		}
	}
	ctx, cancel := context.WithTimeout(ctx, 2*time.Second)
	defer cancel()
	var entries []claudeEntry
	excluded := 0
	seen := map[string]int{}
	conflicts := 0
	files := map[string]bool{}
	for _, sub := range []string{"sessions", "archived_sessions"} {
		parser := codexParser{}
		_, bad, err := readJSONHistoryFilesReset(ctx, filepath.Join(dir, sub), true, func(line []byte) ([]historyEvent, bool) {
			e, ok := parser.parse(line)
			if e == nil {
				return nil, ok
			}
			measurementID := e.event.id
			if e.event.id == "" {
				e.event.id = fmt.Sprintf("invalid-%d", len(entries))
				measurementID = e.event.id
			} else {
				// Contradictory tiers on the same copied measurement remain incomplete.
				measurementID = e.event.id[:strings.LastIndex(e.event.id, "|")]
			}
			if _, found := seen[measurementID]; found {
				return nil, ok
			}
			seen[measurementID] = len(entries)
			entries = append(entries, *e)
			return []historyEvent{e.event}, ok
		}, func(relative string) bool {
			parser = codexParser{}
			if files[relative] {
				return false
			}
			files[relative] = true
			return true
		})
		if err != nil || len(entries) > 10000 {
			h.Status = "unavailable"
			return h
		}
		excluded += bad
	}
	events := make([]historyEvent, 0, len(entries))
	for _, e := range entries {
		events = append(events, e.event)
	}
	aggregateHistory(h, events, excluded+conflicts, now)
	return h
}
