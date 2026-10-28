package usage

import (
	"context"
	_ "embed"
	"encoding/json"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"
)

//go:embed claude_prices.json
var claudePrices []byte

type claudeRate struct {
	Input      uint64  `json:"input"`
	Output     uint64  `json:"output"`
	Write      uint64  `json:"write"`
	Read       uint64  `json:"read"`
	InputLong  *uint64 `json:"input_long"`
	OutputLong *uint64 `json:"output_long"`
	WriteLong  *uint64 `json:"write_long"`
	ReadLong   *uint64 `json:"read_long"`
	Fast       *uint64 `json:"fast"`
}
type claudePriceTable struct {
	Snapshot string                `json:"snapshot"`
	Rates    map[string]claudeRate `json:"rates"`
	Aliases  map[string]string     `json:"aliases"`
}
type claudeEntry struct {
	event                    historyEvent
	message, request         string
	side, speed, unsupported bool
	estimate                 *uint64
}

func claudeHistoryHome() string {
	if dir := os.Getenv("CLAUDE_CONFIG_DIR"); dir != "" {
		return dir
	}
	home, _ := os.UserHomeDir()
	return filepath.Join(home, ".claude")
}

func parseClaudeLine(line []byte, prices claudePriceTable) (*claudeEntry, bool) {
	var obj struct {
		Type      string          `json:"type"`
		Timestamp string          `json:"timestamp"`
		Request   string          `json:"requestId"`
		Side      json.RawMessage `json:"isSidechain"`
		Cost      json.RawMessage `json:"costUSD"`
		Message   struct {
			ID    string                     `json:"id"`
			Role  string                     `json:"role"`
			Model string                     `json:"model"`
			Stop  string                     `json:"stop_reason"`
			Usage map[string]json.RawMessage `json:"usage"`
		} `json:"message"`
	}
	if json.Unmarshal(line, &obj) != nil {
		return nil, false
	}
	if obj.Type != "assistant" {
		return nil, true
	}
	m := obj.Message
	if m.Role != "assistant" {
		return nil, false
	}
	switch m.Stop {
	case "":
		// Claude Code persists assistant usage without a terminal stop marker.
		// Only measured input/output qualify; message-start fragments do not.
		if len(m.Usage) == 0 {
			return nil, true
		}
		if historyCount(m.Usage["input_tokens"]) == nil || historyCount(m.Usage["output_tokens"]) == nil {
			return nil, false
		}
	case "end_turn", "tool_use", "max_tokens", "stop_sequence":
	default:
		return nil, false
	}
	at, err := time.Parse(time.RFC3339Nano, obj.Timestamp)
	validID := func(s string) bool { return len(s) > 0 && len(s) <= 256 && !strings.ContainsAny(s, "\x00\r\n") }
	if err != nil || !validID(m.ID) || !validID(obj.Request) || !validHistoryModel(m.Model) {
		return nil, false
	}
	var side bool
	if len(obj.Side) > 0 && (string(obj.Side) == "null" || json.Unmarshal(obj.Side, &side) != nil) {
		return nil, false
	}
	e := &claudeEntry{message: m.ID, request: obj.Request, side: side, event: historyEvent{model: m.Model, at: at, ticks: openCodeCost(obj.Cost)}}
	u := m.Usage
	if raw, ok := u["service_tier"]; ok && string(raw) != `"standard"` {
		e.unsupported = true
	}
	get := func(key string, optional bool) *uint64 {
		raw, ok := u[key]
		if !ok && optional {
			n := uint64(0)
			return &n
		}
		n := historyCount(raw)
		if n != nil && *n > 1_000_000_000_000 {
			return nil
		}
		return n
	}
	input, output, read, write := get("input_tokens", false), get("output_tokens", false), get("cache_read_input_tokens", true), get("cache_creation_input_tokens", true)
	one := uint64(0)
	write1 := &one
	if raw, ok := u["cache_creation"]; ok {
		var split map[string]json.RawMessage
		if json.Unmarshal(raw, &split) != nil || split == nil {
			write = nil
		} else {
			write = historyCount(split["ephemeral_5m_input_tokens"])
			write1 = historyCount(split["ephemeral_1h_input_tokens"])
			if write != nil && write1 != nil && (*write > 1_000_000_000_000 || *write1 > 1_000_000_000_000) {
				write = nil
			}
			if aggregate, present := u["cache_creation_input_tokens"]; present && write != nil && write1 != nil {
				n := historyCount(aggregate)
				if n == nil || *n != *write+*write1 {
					write = nil
				}
			}
		}
	}
	var speed string
	if raw, ok := u["speed"]; ok {
		e.speed = true
		if json.Unmarshal(raw, &speed) != nil || (speed != "standard" && speed != "fast") {
			e.unsupported = true
		}
	}
	if raw, ok := u["iterations"]; ok {
		var iterations []struct {
			Type string `json:"type"`
		}
		if string(raw) == "null" || json.Unmarshal(raw, &iterations) != nil {
			e.unsupported = true
		} else {
			for _, v := range iterations {
				if v.Type != "message" {
					e.unsupported = true
				}
			}
		}
	}
	if input != nil && output != nil && read != nil && write != nil && write1 != nil {
		total := *input + *output + *read + *write + *write1
		e.event.tokens = &total
		if len(obj.Cost) == 0 && !e.unsupported {
			model := m.Model
			if alias, ok := prices.Aliases[model]; ok {
				model = alias
			}
			rate, known := prices.Rates[model]
			if known && (speed != "fast" || rate.Fast != nil || strings.HasSuffix(model, "-fast")) {
				rates := []uint64{rate.Input, rate.Output, rate.Read, rate.Write, rate.Input * 2}
				if total-*output > 200_000 {
					for i, v := range []*uint64{rate.InputLong, rate.OutputLong, rate.ReadLong, rate.WriteLong} {
						if v != nil {
							rates[i] = *v
						}
					}
					rates[4] = rates[0] * 2
				}
				ticks := *input*rates[0] + *output*rates[1] + *read*rates[2] + *write*rates[3] + *write1*rates[4]
				if speed == "fast" && rate.Fast != nil {
					ticks *= *rate.Fast
				}
				if ticks <= 1_000_000_000_000_000 {
					e.estimate = &ticks
				}
			}
		}
	}
	return e, true
}

// Prefer canonical parents, then richer completed usage. Models cannot change on a replay.
func dedupClaude(entries []claudeEntry) ([]claudeEntry, int) {
	var result []claudeEntry
	exact := map[string]int{}
	messages := map[string][]int{}
	conflicts := map[int]bool{}
	for _, e := range entries {
		key := e.message + "\x00" + e.request
		index, found := exact[key]
		if !found {
			for _, i := range messages[e.message] {
				if e.side || result[i].side {
					index, found = i, true
					break
				}
			}
		}
		if found {
			old := result[index]
			if old.event.model != e.event.model {
				conflicts[index] = true
				continue
			}
			total := func(v *uint64) uint64 {
				if v == nil {
					return 0
				}
				return *v
			}
			replace := false
			if e.side != old.side {
				replace = old.side
			} else if total(e.event.tokens) != total(old.event.tokens) {
				replace = total(e.event.tokens) > total(old.event.tokens)
			} else if (e.event.ticks == nil) != (old.event.ticks == nil) {
				replace = e.event.ticks != nil
			} else {
				replace = e.speed && !old.speed
			}
			if replace {
				delete(exact, old.message+"\x00"+old.request)
				result[index] = e
				exact[key] = index
			}
		} else {
			exact[key] = len(result)
			messages[e.message] = append(messages[e.message], len(result))
			result = append(result, e)
		}
	}
	accepted := make([]claudeEntry, 0, len(result))
	for i, e := range result {
		if !conflicts[i] {
			e.event.id = strconv.Itoa(i)
			accepted = append(accepted, e)
		}
	}
	return accepted, len(conflicts)
}

// ScanClaudeHistory reads selected completed Claude Code records without credentials or source writes.
func ScanClaudeHistory(ctx context.Context, dir string, now time.Time) *LocalHistory {
	var prices claudePriceTable
	h := &LocalHistory{Scope: "local_machine", Attribution: "unattributed", Currency: "USD", CostBasis: "unknown", Timezone: now.Location().String(), Status: "no_data", Daily: []HistoryBucket{}, Periods: []HistoryBucket{}}
	if now.Location() == time.Local {
		h.Timezone = "local"
	}
	if json.Unmarshal(claudePrices, &prices) != nil {
		h.Status = "unavailable"
		return h
	}
	h.PriceSnapshot = &prices.Snapshot
	if !filepath.IsAbs(dir) || strings.Contains(dir, ",") {
		h.Status = "unavailable"
		return h
	}
	for _, part := range strings.Split(dir, "/") {
		if part == "." || part == ".." {
			h.Status = "unavailable"
			return h
		}
	}
	var entries []claudeEntry
	_, excluded, err := readJSONHistoryFiles(ctx, filepath.Join(dir, "projects"), true, func(line []byte) ([]historyEvent, bool) {
		e, ok := parseClaudeLine(line, prices)
		if e == nil {
			return nil, ok
		}
		entries = append(entries, *e)
		return []historyEvent{e.event}, ok
	})
	if err != nil {
		h.Status = "unavailable"
		return h
	}
	entries, conflicts := dedupClaude(entries)
	excluded += conflicts
	events := make([]historyEvent, 0, len(entries))
	for _, e := range entries {
		events = append(events, e.event)
		if e.unsupported {
			excluded++
		}
	}
	aggregateHistory(h, events, excluded, now)
	applyClaudeCosts(h, entries, now)
	return h
}
