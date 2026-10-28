package usage

import (
	"strconv"
	"strings"
	"time"
)

type claudeCostSum struct {
	recorded, estimated         uint64
	records, estimates, missing int
}

func claudeAmount(ticks uint64) *string {
	s := strconv.FormatUint(ticks/10_000_000_000, 10)
	if ticks%10_000_000_000 != 0 {
		s += "." + strings.TrimRight(strconv.FormatUint(ticks%10_000_000_000+10_000_000_000, 10)[1:], "0")
	}
	return &s
}
func claudeBasis(recorded, estimated int) string {
	if recorded > 0 && estimated > 0 {
		return "mixed"
	}
	if recorded > 0 {
		return "recorded"
	}
	if estimated > 0 {
		return "estimated"
	}
	return "unknown"
}
func applyClaudeCosts(h *LocalHistory, entries []claudeEntry, now time.Time) {
	sums := map[string]map[string]*claudeCostSum{}
	today := now.Format("2006-01-02")
	yesterday := now.AddDate(0, 0, -1).Format("2006-01-02")
	since := now.AddDate(0, 0, -29).Format("2006-01-02")
	records, estimates := 0, 0
	for _, e := range entries {
		day := e.event.at.In(now.Location()).Format("2006-01-02")
		if day < since || e.event.at.After(now) {
			continue
		}
		keys := []string{"d:" + day, "p:last_30_days"}
		if day == today {
			keys = append(keys, "p:today")
		}
		if day == yesterday {
			keys = append(keys, "p:yesterday")
		}
		if e.event.ticks != nil {
			records++
		} else if e.estimate != nil {
			estimates++
		}
		for _, k := range keys {
			if sums[k] == nil {
				sums[k] = map[string]*claudeCostSum{}
			}
			if sums[k][e.event.model] == nil {
				sums[k][e.event.model] = &claudeCostSum{}
			}
			s := sums[k][e.event.model]
			if e.event.ticks != nil {
				s.recorded += *e.event.ticks
				s.records++
			} else if e.estimate != nil {
				s.estimated += *e.estimate
				s.estimates++
			} else {
				s.missing++
			}
		}
	}
	h.CostBasis = claudeBasis(records, estimates)
	h.Status = "no_data"
	if len(h.Daily) > 0 {
		h.Status = "available"
	}
	if h.ExcludedEvents > 0 {
		h.Status = "partial"
	}
	for _, group := range []struct {
		prefix  string
		buckets []HistoryBucket
	}{{"d:", h.Daily}, {"p:", h.Periods}} {
		for i := range group.buckets {
			b := &group.buckets[i]
			for j := range b.Models {
				m := &b.Models[j]
				s := sums[group.prefix+b.Key][m.Model]
				basis := claudeBasis(s.records, s.estimates)
				m.CostBasis = &basis
				m.RecordedEvents = &s.records
				m.EstimatedEvents = &s.estimates
				m.MissingCost = s.missing
				m.RecordedCost = nil
				if s.records > 0 {
					m.RecordedCost = claudeAmount(s.recorded)
				}
				if s.estimates > 0 {
					m.EstimatedCost = claudeAmount(s.estimated)
				}
				if s.missing > 0 || m.MissingTokens > 0 {
					h.Status = "partial"
				}
			}
		}
	}
}
