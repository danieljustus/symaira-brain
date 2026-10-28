package usage

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"time"
)

// LocalHistory is independent of quota credentials, balances and account identity.
type LocalHistory struct {
	Scope          string          `json:"scope"`
	Attribution    string          `json:"attribution"`
	Currency       string          `json:"currency"`
	CostBasis      string          `json:"cost_basis"`
	Timezone       string          `json:"timezone"`
	Status         string          `json:"status"`
	ExcludedEvents int             `json:"excluded_events"`
	Daily          []HistoryBucket `json:"daily"`
	Periods        []HistoryBucket `json:"periods"`
}

type HistoryBucket struct {
	Key    string         `json:"key"`
	Models []HistoryModel `json:"models"`
}

// Amounts are absent unless every accepted contribution has that measurement.
type HistoryModel struct {
	Model         string  `json:"model"`
	Events        int     `json:"events"`
	Tokens        *string `json:"tokens,omitempty"`
	RecordedCost  *string `json:"recorded_cost,omitempty"`
	MissingTokens int     `json:"missing_tokens"`
	MissingCost   int     `json:"missing_cost"`
}

type historyEvent struct {
	id, model     string
	at            time.Time
	tokens, ticks *uint64
}

func historyCount(v json.RawMessage) *uint64 {
	if len(v) == 0 {
		return nil
	}
	n, err := strconv.ParseUint(string(v), 10, 64)
	if err != nil || n > 1_000_000_000_000_000 {
		return nil
	}
	return &n
}

func parseHistoryLine(line []byte) ([]historyEvent, bool) {
	var obj struct {
		Timestamp json.RawMessage  `json:"timestamp"`
		Params    *historyEnvelope `json:"params"`
		historyEnvelope
	}
	if json.Unmarshal(line, &obj) != nil {
		return nil, false
	}
	envelope := &obj.historyEnvelope
	if obj.Params != nil {
		envelope = obj.Params
	}
	if envelope.Update.Kind != "turn_completed" {
		return nil, true
	}
	var at time.Time
	if n := historyCount(envelope.Meta.Timestamp); n != nil && *n > 0 && *n < 253402300800000 {
		at = time.UnixMilli(int64(*n))
	} else {
		var text string
		if json.Unmarshal(obj.Timestamp, &text) == nil {
			at, _ = time.Parse(time.RFC3339Nano, text)
		} else if n := historyCount(obj.Timestamp); n != nil && *n > 0 && *n < 253402300800 {
			at = time.Unix(int64(*n), 0)
		}
	}
	id := strings.TrimSpace(envelope.Meta.ID)
	models := envelope.Update.Usage.Models
	if at.IsZero() || len(id) == 0 || len(id) > 256 || len(models) == 0 || len(models) > 32 {
		return nil, false
	}
	result := make([]historyEvent, 0, len(models))
	for model, values := range models {
		if !validHistoryModel(model) {
			return nil, false
		}
		var valuesObj struct {
			Input  json.RawMessage `json:"inputTokens"`
			Output json.RawMessage `json:"outputTokens"`
			Cost   json.RawMessage `json:"costUsdTicks"`
		}
		if string(values) == "null" || json.Unmarshal(values, &valuesObj) != nil {
			return nil, false
		}
		e := historyEvent{id: id, model: model, at: at, ticks: historyCount(valuesObj.Cost)}
		if len(valuesObj.Cost) == 0 && len(models) == 1 {
			e.ticks = historyCount(envelope.Update.Usage.Cost)
		}
		input, output := historyCount(valuesObj.Input), historyCount(valuesObj.Output)
		if input != nil && output != nil && *input <= 1_000_000_000_000 && *output <= 1_000_000_000_000 {
			total := *input + *output
			e.tokens = &total
		}
		result = append(result, e)
	}
	return result, true
}

type historyEnvelope struct {
	Meta struct {
		ID        string          `json:"eventId"`
		Timestamp json.RawMessage `json:"agentTimestampMs"`
	} `json:"_meta"`
	Update struct {
		Kind  string `json:"sessionUpdate"`
		Usage struct {
			Models map[string]json.RawMessage `json:"modelUsage"`
			Cost   json.RawMessage            `json:"costUsdTicks"`
		} `json:"usage"`
	} `json:"update"`
}

func validHistoryModel(model string) bool {
	if len(model) == 0 || len(model) > 128 || strings.Contains(model, "..") || !(model[0] >= 'a' && model[0] <= 'z' || model[0] >= 'A' && model[0] <= 'Z' || model[0] >= '0' && model[0] <= '9') {
		return false
	}
	for _, c := range model {
		if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || strings.ContainsRune("-_.:/", c)) {
			return false
		}
	}
	return true
}

// ScanGrokHistory reads only explicitly opted-in local completed events. No credential I/O.
func ScanGrokHistory(ctx context.Context, home string, now time.Time) *LocalHistory {
	h := &LocalHistory{Scope: "local_machine", Attribution: "unattributed", Currency: "USD", CostBasis: "recorded", Timezone: now.Location().String(), Status: "no_data", Daily: []HistoryBucket{}, Periods: []HistoryBucket{}}
	if now.Location() == time.Local {
		h.Timezone = "local"
	}
	if !filepath.IsAbs(home) {
		h.Status = "unavailable"
		return h
	}
	for _, part := range strings.Split(home, "/") {
		if part == "." || part == ".." {
			h.Status = "unavailable"
			return h
		}
	}
	events, excluded, err := readHistoryFiles(ctx, filepath.Join(home, "sessions"))
	if err != nil {
		h.Status = "unavailable"
		return h
	}
	h.ExcludedEvents = excluded
	unique := map[string]historyEvent{}
	conflicts := map[string]bool{}
	for _, e := range events {
		key := e.id + "\x00" + e.model
		if old, ok := unique[key]; ok {
			if !sameHistoryEvent(old, e) {
				conflicts[key] = true
			}
		} else {
			unique[key] = e
		}
	}
	today := now.Format("2006-01-02")
	yesterday := now.AddDate(0, 0, -1).Format("2006-01-02")
	since := now.AddDate(0, 0, -29).Format("2006-01-02")
	buckets := map[string]map[string][]historyEvent{}
	for key, e := range unique {
		if conflicts[key] {
			h.ExcludedEvents++
			continue
		}
		day := e.at.In(now.Location()).Format("2006-01-02")
		if day < since || e.at.After(now) {
			continue
		}
		keys := []string{"d:" + day, "p:last_30_days"}
		if day == today {
			keys = append(keys, "p:today")
		}
		if day == yesterday {
			keys = append(keys, "p:yesterday")
		}
		for _, k := range keys {
			if buckets[k] == nil {
				buckets[k] = map[string][]historyEvent{}
			}
			buckets[k][e.model] = append(buckets[k][e.model], e)
		}
	}
	keys := make([]string, 0, len(buckets))
	for k := range buckets {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	for _, key := range keys {
		b := HistoryBucket{Key: key[2:], Models: []HistoryModel{}}
		models := make([]string, 0, len(buckets[key]))
		for m := range buckets[key] {
			models = append(models, m)
		}
		sort.Strings(models)
		for _, m := range models {
			b.Models = append(b.Models, sumHistory(m, buckets[key][m]))
		}
		if strings.HasPrefix(key, "d:") {
			h.Daily = append(h.Daily, b)
		} else {
			h.Periods = append(h.Periods, b)
		}
	}
	if len(h.Daily) > 0 {
		h.Status = "available"
	}
	for _, bucket := range h.Daily {
		for _, model := range bucket.Models {
			if model.MissingCost > 0 || model.MissingTokens > 0 {
				h.Status = "partial"
			}
		}
	}
	if h.ExcludedEvents > 0 {
		h.Status = "partial"
	}
	return h
}

func sameHistoryEvent(a, b historyEvent) bool {
	eq := func(a, b *uint64) bool { return a == nil && b == nil || a != nil && b != nil && *a == *b }
	return a.at.Equal(b.at) && eq(a.tokens, b.tokens) && eq(a.ticks, b.ticks)
}
func sumHistory(model string, events []historyEvent) HistoryModel {
	m := HistoryModel{Model: model, Events: len(events)}
	var tokens, ticks uint64
	for _, e := range events {
		if e.tokens == nil {
			m.MissingTokens++
		} else {
			tokens += *e.tokens
		}
		if e.ticks == nil {
			m.MissingCost++
		} else {
			ticks += *e.ticks
		}
	}
	if m.MissingTokens == 0 {
		s := strconv.FormatUint(tokens, 10)
		m.Tokens = &s
	}
	if m.MissingCost == 0 {
		s := strconv.FormatUint(ticks/10_000_000_000, 10)
		if ticks%10_000_000_000 != 0 {
			fraction := strconv.FormatUint(ticks%10_000_000_000+10_000_000_000, 10)[1:]
			s += "." + strings.TrimRight(fraction, "0")
		}
		m.RecordedCost = &s
	}
	return m
}

func grokHistoryHome() string {
	if home := strings.TrimSpace(os.Getenv("GROK_HOME")); home != "" {
		return home
	}
	home, _ := os.UserHomeDir()
	return filepath.Join(home, ".grok")
}
