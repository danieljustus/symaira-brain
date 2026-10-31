package usage

import (
	"encoding/json"
	"math"
	"strconv"
	"time"
)

func parseOllamaBalance(body []byte, now time.Time) (*UsageSnapshot, error) {
	var root map[string]json.RawMessage
	if json.Unmarshal(body, &root) != nil {
		return nil, &PayloadError{ProviderID: "ollama", Detail: "invalid balance response"}
	}
	var included map[string]json.RawMessage
	_ = json.Unmarshal(root["included"], &included)
	snap := &UsageSnapshot{ProviderID: "ollama", Meters: []UsageMeter{}, Source: "api", FetchedAt: now}
	for _, window := range []struct{ key, label string }{{"session", "Session"}, {"weekly", "Weekly"}} {
		var entry map[string]json.RawMessage
		_ = json.Unmarshal(included[window.key], &entry)
		if remaining, ok := ollamaNumber(entry["remaining_percent"]); ok {
			snap.Meters = append(snap.Meters, UsageMeter{Label: window.label, Used: strPtr(ollamaAmount(math.Max(0, math.Min(100, 100-remaining)))), Limit: strPtr("100"), Unit: "%", ResetsAt: ollamaDate(entry["resets_at"])})
		}
	}
	allowance, aok := ollamaNumber(included["allowance_usd"])
	balance, bok := ollamaNumber(included["balance_usd"])
	if aok && bok && allowance > 0 {
		var period map[string]json.RawMessage
		_ = json.Unmarshal(included["period"], &period)
		snap.Meters = append(snap.Meters, UsageMeter{Label: "Monthly", Used: strPtr(ollamaAmount(math.Min(allowance, math.Max(0, allowance-balance)))), Limit: strPtr(ollamaAmount(allowance)), Unit: "USD", ResetsAt: ollamaDate(period["until"])})
	}
	if len(snap.Meters) == 0 {
		return nil, &PayloadError{ProviderID: "ollama", Detail: "response contained no usable included quota"}
	}
	var purchased map[string]json.RawMessage
	_ = json.Unmarshal(root["purchased"], &purchased)
	if value, ok := ollamaNumber(purchased["balance_usd"]); ok {
		snap.Balance = strPtr(ollamaAmount(math.Max(0, value)))
		snap.Currency = strPtr("USD")
	}
	return snap, nil
}
func ollamaNumber(raw json.RawMessage) (float64, bool) {
	if len(raw) == 0 || string(raw) == "null" {
		return 0, false
	}
	var text string
	if json.Unmarshal(raw, &text) != nil {
		text = string(raw)
	}
	value, err := strconv.ParseFloat(text, 64)
	return value, err == nil && !math.IsNaN(value) && !math.IsInf(value, 0)
}
func ollamaAmount(value float64) string {
	if value == 0 {
		return "0"
	}
	return strconv.FormatFloat(value, 'f', -1, 64)
}
func ollamaDate(raw json.RawMessage) *time.Time {
	var text string
	if json.Unmarshal(raw, &text) != nil {
		return nil
	}
	date, err := time.Parse(time.RFC3339Nano, text)
	if err != nil {
		return nil
	}
	return &date
}
