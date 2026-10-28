package usage

import (
	"context"
	"encoding/json"
	"math/big"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"
)

func openCodeHistoryHome() string {
	if dir := strings.TrimSpace(os.Getenv("OPENCODE_DATA_DIR")); dir != "" {
		return dir
	}
	base := strings.TrimSpace(os.Getenv("XDG_DATA_HOME"))
	if base == "" {
		home, _ := os.UserHomeDir()
		base = filepath.Join(home, ".local", "share")
	}
	return filepath.Join(base, "opencode")
}

// ScanOpenCodeHistory unions recorded hosted traffic, never current-account credentials.
func ScanOpenCodeHistory(ctx context.Context, dir string, now time.Time) *LocalHistory {
	h := &LocalHistory{Scope: "local_machine", Attribution: "unattributed", Currency: "USD", CostBasis: "recorded", Timezone: now.Location().String(), Status: "no_data", Daily: []HistoryBucket{}, Periods: []HistoryBucket{}}
	if now.Location() == time.Local {
		h.Timezone = "local"
	}
	if !filepath.IsAbs(dir) {
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
	events, excluded, err := readOpenCodeHistory(ctx, dir)
	if err != nil {
		h.Status = "unavailable"
		return h
	}
	return aggregateHistory(h, events, excluded, now)
}

// Exact recorded decimal to the shared 10^-10 USD unit. Unsupported precision stays unknown.
func openCodeCost(raw json.RawMessage) *uint64 {
	if len(raw) == 0 || len(raw) > 64 || raw[0] == '"' {
		return nil
	}
	if i := strings.IndexAny(string(raw), "eE"); i >= 0 {
		exponent, err := strconv.ParseInt(string(raw[i+1:]), 10, 16)
		if err != nil || exponent < -128 || exponent > 128 {
			return nil
		}
	}
	r, ok := new(big.Rat).SetString(string(raw))
	if !ok || r.Sign() < 0 {
		return nil
	}
	r.Mul(r, big.NewRat(10_000_000_000, 1))
	if !r.IsInt() || !r.Num().IsUint64() {
		return nil
	}
	n := r.Num().Uint64()
	if n > 1_000_000_000_000_000 {
		return nil
	}
	return &n
}

type openCodeHistoryRow struct {
	id, provider string
	event        historyEvent
	hosted       bool
}

func parseOpenCodeHistoryRow(id string, ms int64, kind string, data []byte) (openCodeHistoryRow, bool) {
	var obj struct {
		Role       string `json:"role"`
		Status     string `json:"status"`
		ModelID    string `json:"modelID"`
		ProviderID string `json:"providerID"`
		Model      *struct {
			ID         string `json:"id"`
			ProviderID string `json:"providerID"`
		} `json:"model"`
		Cost   json.RawMessage `json:"cost"`
		Tokens struct {
			Total     json.RawMessage `json:"total"`
			Input     json.RawMessage `json:"input"`
			Output    json.RawMessage `json:"output"`
			Reasoning json.RawMessage `json:"reasoning"`
			Cache     struct {
				Read  json.RawMessage `json:"read"`
				Write json.RawMessage `json:"write"`
			} `json:"cache"`
		} `json:"tokens"`
	}
	if json.Unmarshal(data, &obj) != nil {
		return openCodeHistoryRow{}, false
	}
	if kind == "" {
		kind = obj.Role
	}
	if kind != "assistant" && !(kind == "compaction" && obj.Status == "completed") {
		return openCodeHistoryRow{}, true
	}
	model, provider := obj.ModelID, obj.ProviderID
	if obj.Model != nil {
		if model != "" && model != obj.Model.ID || provider != "" && provider != obj.Model.ProviderID {
			return openCodeHistoryRow{}, false
		}
		model, provider = obj.Model.ID, obj.Model.ProviderID
	}
	if id == "" || len(id) > 256 || ms <= 0 || ms >= 253402300800000 || !validHistoryModel(model) || provider == "" {
		return openCodeHistoryRow{}, false
	}
	e := historyEvent{id: id, model: model, at: time.UnixMilli(ms), ticks: openCodeCost(obj.Cost)}
	if len(obj.Tokens.Total) > 0 {
		e.tokens = historyCount(obj.Tokens.Total)
	} else {
		parts := []json.RawMessage{obj.Tokens.Input, obj.Tokens.Output, obj.Tokens.Reasoning, obj.Tokens.Cache.Read, obj.Tokens.Cache.Write}
		var total uint64
		valid := true
		for _, part := range parts {
			n := historyCount(part)
			if n == nil || *n > 1_000_000_000_000 {
				valid = false
				break
			}
			total += *n
		}
		if valid {
			e.tokens = &total
		}
	}
	return openCodeHistoryRow{id: id, provider: provider, event: e, hosted: provider == "opencode-go" || provider == "opencode"}, true
}

func dedupOpenCodeHistory(rows []openCodeHistoryRow, excluded int) ([]historyEvent, int) {
	unique := map[string]openCodeHistoryRow{}
	conflicts := map[string]bool{}
	for _, row := range rows {
		if row.id == "" {
			continue
		}
		if old, ok := unique[row.id]; ok {
			if old.provider != row.provider || old.event.model != row.event.model || !sameHistoryEvent(old.event, row.event) {
				conflicts[row.id] = true
			}
		} else {
			unique[row.id] = row
		}
	}
	events := []historyEvent{}
	for id, row := range unique {
		if conflicts[id] {
			excluded++
			continue
		}
		if row.hosted {
			events = append(events, row.event)
		}
	}
	return events, excluded
}
