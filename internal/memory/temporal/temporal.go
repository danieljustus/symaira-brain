// Package temporal extracts time-window constraints from natural-language
// English and German time expressions found in search queries.
//
// Supported expressions (case-insensitive):
//
//	English: "yesterday", "today", "last week", "last month", "last year",
//	         "this week", "this month", "this year",
//	         "in the last N days/weeks/months/years",
//	         "in the past N days/weeks/months/years",
//	         "N days/weeks/months/years ago"
//	German:  "gestern", "heute", "letzte Woche", "letzten Monat", "letztes Jahr",
//	         "diese Woche", "diesen Monat", "dieses Jahr",
//	         "in den letzten N Tagen/Wochen/Monaten/Jahren",
//	         "vor N Tagen/Wochen/Monaten/Jahren"
//
// When no temporal expression is found, Extract returns a zero-value TimeWindow
// and a nil error, which the caller treats as "no constraint".
package temporal

import (
	"regexp"
	"strings"
	"time"
)

// TimeWindow mirrors the db.TimeWindow type so this package has no dependency
// on the db package. Callers convert via its pointer fields.
type TimeWindow struct {
	From *time.Time
	To   *time.Time
}

// Result holds the optional time-window constraint extracted from a query.
type Result struct {
	Window      TimeWindow
	Confidence  string // "high" for explicit expressions, "low" for weak/vague
	MatchedText string // the text fragment that triggered extraction
}

var (
	// English patterns ordered by specificity (most specific first).
	enYesterday   = regexp.MustCompile(`(?i)\byesterday\b`)
	enToday       = regexp.MustCompile(`(?i)\btoday\b`)
	enLastWeek    = regexp.MustCompile(`(?i)\blast\s+week\b`)
	enLastMonth   = regexp.MustCompile(`(?i)\blast\s+month\b`)
	enLastYear    = regexp.MustCompile(`(?i)\blast\s+year\b`)
	enThisWeek    = regexp.MustCompile(`(?i)\bthis\s+week\b`)
	enThisMonth   = regexp.MustCompile(`(?i)\bthis\s+month\b`)
	enThisYear    = regexp.MustCompile(`(?i)\bthis\s+year\b`)
	enLastNDays   = regexp.MustCompile(`(?i)(?:in\s+)?(?:the\s+)?(?:last|past)\s+(\d+)\s+days?\b`)
	enLastNWeeks  = regexp.MustCompile(`(?i)(?:in\s+)?(?:the\s+)?(?:last|past)\s+(\d+)\s+weeks?\b`)
	enLastNMonths = regexp.MustCompile(`(?i)(?:in\s+)?(?:the\s+)?(?:last|past)\s+(\d+)\s+months?\b`)
	enLastNYears  = regexp.MustCompile(`(?i)(?:in\s+)?(?:the\s+)?(?:last|past)\s+(\d+)\s+years?\b`)
	enNDaysAgo    = regexp.MustCompile(`(?i)(\d+)\s+days?\s+ago\b`)
	enNWeeksAgo   = regexp.MustCompile(`(?i)(\d+)\s+weeks?\s+ago\b`)
	enNMonthsAgo  = regexp.MustCompile(`(?i)(\d+)\s+months?\s+ago\b`)
	enNYearsAgo   = regexp.MustCompile(`(?i)(\d+)\s+years?\s+ago\b`)

	// German patterns.
	deGestern        = regexp.MustCompile(`(?i)\bgestern\b`)
	deHeute          = regexp.MustCompile(`(?i)\bheute\b`)
	deLetzteWoche    = regexp.MustCompile(`(?i)\bletzt(?:e|en)\s+woche\b`)
	deLetzterMonat   = regexp.MustCompile(`(?i)\bletzt(?:e|en|er)\s+monat\b`)
	deLetztesJahr    = regexp.MustCompile(`(?i)\bletzt(?:e|es)\s+jahr\b`)
	deDieseWoche     = regexp.MustCompile(`(?i)\bdiese\s+woche\b`)
	deDiesenMonat    = regexp.MustCompile(`(?i)\bdiese(?:n|s|)\s+monat\b`)
	deDiesesJahr     = regexp.MustCompile(`(?i)\bdiese(?:s|)\s+jahr\b`)
	deLetztenNTage   = regexp.MustCompile(`(?i)(?:in\s+)?den\s+letzten\s+(\d+)\s+tagen?\b`)
	deLetztenNWochen = regexp.MustCompile(`(?i)(?:in\s+)?den\s+letzten\s+(\d+)\s+wochen?\b`)
	deLetztenNMonate = regexp.MustCompile(`(?i)(?:in\s+)?den\s+letzten\s+(\d+)\s+monaten?\b`)
	deLetztenNJahre  = regexp.MustCompile(`(?i)(?:in\s+)?den\s+letzten\s+(\d+)\s+jahren?\b`)
	deVorNTage       = regexp.MustCompile(`(?i)\bvor\s+(\d+)\s+tage?n?\b`)
	deVorNWochen     = regexp.MustCompile(`(?i)\bvor\s+(\d+)\s+wochen?\b`)
	deVorNMonaten    = regexp.MustCompile(`(?i)\bvor\s+(\d+)\s+monate?n?\b`)
	deVorNJahren     = regexp.MustCompile(`(?i)\bvor\s+(\d+)\s+jahre?n?\b`)
)

// Extract scans s for common English and German time expressions and, when one
// is found, returns a Result with the corresponding time-window constraint.
// For expression like "last week" the window covers the week that just ended;
// for "yesterday" the window covers the previous calendar day, etc.
//
// When no expression is matched, Result is zero-valued and error is nil.
// Negative cases (weak signals such as "tomorrow", "soon", "bald") are
// intentionally excluded.
func Extract(s string, now time.Time) *Result {
	s = strings.TrimSpace(s)
	if s == "" {
		return nil
	}

	// Named day/week/month/year matches (highest priority, checked first).
	// Single-point expressions like "yesterday" produce a To-bound only
	// (valid_from <= yesterday-end).
	if m := enYesterday.FindString(s); m != "" {
		start := time.Date(now.Year(), now.Month(), now.Day()-1, 0, 0, 0, 0, time.UTC)
		end := start.Add(24 * time.Hour)
		return &Result{
			Window:      TimeWindow{From: &start, To: &end},
			Confidence:  "high",
			MatchedText: m,
		}
	}
	if m := deGestern.FindString(s); m != "" {
		start := time.Date(now.Year(), now.Month(), now.Day()-1, 0, 0, 0, 0, time.UTC)
		end := start.Add(24 * time.Hour)
		return &Result{
			Window:      TimeWindow{From: &start, To: &end},
			Confidence:  "high",
			MatchedText: m,
		}
	}
	if m := enToday.FindString(s); m != "" {
		start := time.Date(now.Year(), now.Month(), now.Day(), 0, 0, 0, 0, time.UTC)
		end := start.Add(24 * time.Hour)
		return &Result{
			Window:      TimeWindow{From: &start, To: &end},
			Confidence:  "high",
			MatchedText: m,
		}
	}
	if m := deHeute.FindString(s); m != "" {
		start := time.Date(now.Year(), now.Month(), now.Day(), 0, 0, 0, 0, time.UTC)
		end := start.Add(24 * time.Hour)
		return &Result{
			Window:      TimeWindow{From: &start, To: &end},
			Confidence:  "high",
			MatchedText: m,
		}
	}

	// Relative named periods: "last week" → previous calendar week, etc.
	if m := enLastWeek.FindString(s); m != "" {
		return lastWeekResult(m, now, time.UTC)
	}
	if m := deLetzteWoche.FindString(s); m != "" {
		return lastWeekResult(m, now, time.UTC)
	}
	if m := enThisWeek.FindString(s); m != "" {
		start := startOfWeek(now, time.UTC)
		end := start.Add(7 * 24 * time.Hour)
		return &Result{
			Window:      TimeWindow{From: &start, To: &end},
			Confidence:  "high",
			MatchedText: m,
		}
	}
	if m := deDieseWoche.FindString(s); m != "" {
		start := startOfWeek(now, time.UTC)
		end := start.Add(7 * 24 * time.Hour)
		return &Result{
			Window:      TimeWindow{From: &start, To: &end},
			Confidence:  "high",
			MatchedText: m,
		}
	}
	if m := enLastMonth.FindString(s); m != "" {
		return lastMonthResult(m, now, time.UTC)
	}
	if m := deLetzterMonat.FindString(s); m != "" {
		return lastMonthResult(m, now, time.UTC)
	}
	if m := enThisMonth.FindString(s); m != "" {
		start := time.Date(now.Year(), now.Month(), 1, 0, 0, 0, 0, time.UTC)
		end := start.AddDate(0, 1, 0)
		return &Result{
			Window:      TimeWindow{From: &start, To: &end},
			Confidence:  "high",
			MatchedText: m,
		}
	}
	if m := deDiesenMonat.FindString(s); m != "" {
		start := time.Date(now.Year(), now.Month(), 1, 0, 0, 0, 0, time.UTC)
		end := start.AddDate(0, 1, 0)
		return &Result{
			Window:      TimeWindow{From: &start, To: &end},
			Confidence:  "high",
			MatchedText: m,
		}
	}
	if m := enLastYear.FindString(s); m != "" {
		return lastYearResult(m, now, time.UTC)
	}
	if m := deLetztesJahr.FindString(s); m != "" {
		return lastYearResult(m, now, time.UTC)
	}
	if m := enThisYear.FindString(s); m != "" {
		start := time.Date(now.Year(), 1, 1, 0, 0, 0, 0, time.UTC)
		end := start.AddDate(1, 0, 0)
		return &Result{
			Window:      TimeWindow{From: &start, To: &end},
			Confidence:  "high",
			MatchedText: m,
		}
	}
	if m := deDiesesJahr.FindString(s); m != "" {
		start := time.Date(now.Year(), 1, 1, 0, 0, 0, 0, time.UTC)
		end := start.AddDate(1, 0, 0)
		return &Result{
			Window:      TimeWindow{From: &start, To: &end},
			Confidence:  "high",
			MatchedText: m,
		}
	}

	// "in the last N days/weeks/months/years" → From = now - N duration
	if matches := enLastNDays.FindStringSubmatch(s); len(matches) > 1 {
		return nDurationResult(matches[0], matches[1], now, hoursPerDay, "high")
	}
	if matches := deLetztenNTage.FindStringSubmatch(s); len(matches) > 1 {
		return nDurationResult(matches[0], matches[1], now, hoursPerDay, "high")
	}
	if matches := enLastNWeeks.FindStringSubmatch(s); len(matches) > 1 {
		return nDurationResult(matches[0], matches[1], now, hoursPerWeek, "high")
	}
	if matches := deLetztenNWochen.FindStringSubmatch(s); len(matches) > 1 {
		return nDurationResult(matches[0], matches[1], now, hoursPerWeek, "high")
	}
	if matches := enLastNMonths.FindStringSubmatch(s); len(matches) > 1 {
		return nDurationResult(matches[0], matches[1], now, hoursPerMonth, "high")
	}
	if matches := deLetztenNMonate.FindStringSubmatch(s); len(matches) > 1 {
		return nDurationResult(matches[0], matches[1], now, hoursPerMonth, "high")
	}
	if matches := enLastNYears.FindStringSubmatch(s); len(matches) > 1 {
		return nDurationResult(matches[0], matches[1], now, hoursPerYear, "high")
	}
	if matches := deLetztenNJahre.FindStringSubmatch(s); len(matches) > 1 {
		return nDurationResult(matches[0], matches[1], now, hoursPerYear, "high")
	}

	// "N days/weeks/months/years ago" → From = now - N duration
	if matches := enNDaysAgo.FindStringSubmatch(s); len(matches) > 1 {
		return nDurationResult(matches[0], matches[1], now, hoursPerDay, "high")
	}
	if matches := deVorNTage.FindStringSubmatch(s); len(matches) > 1 {
		return nDurationResult(matches[0], matches[1], now, hoursPerDay, "high")
	}
	if matches := enNWeeksAgo.FindStringSubmatch(s); len(matches) > 1 {
		return nDurationResult(matches[0], matches[1], now, hoursPerWeek, "high")
	}
	if matches := deVorNWochen.FindStringSubmatch(s); len(matches) > 1 {
		return nDurationResult(matches[0], matches[1], now, hoursPerWeek, "high")
	}
	if matches := enNMonthsAgo.FindStringSubmatch(s); len(matches) > 1 {
		return nDurationResult(matches[0], matches[1], now, hoursPerMonth, "high")
	}
	if matches := deVorNMonaten.FindStringSubmatch(s); len(matches) > 1 {
		return nDurationResult(matches[0], matches[1], now, hoursPerMonth, "high")
	}
	if matches := enNYearsAgo.FindStringSubmatch(s); len(matches) > 1 {
		return nDurationResult(matches[0], matches[1], now, hoursPerYear, "high")
	}
	if matches := deVorNJahren.FindStringSubmatch(s); len(matches) > 1 {
		return nDurationResult(matches[0], matches[1], now, hoursPerYear, "high")
	}

	return nil
}

const (
	hoursPerDay   = 24
	hoursPerWeek  = 7 * 24
	hoursPerMonth = 30 * 24  // approximate
	hoursPerYear  = 365 * 24 // approximate
)

func lastWeekResult(matched string, now time.Time, loc *time.Location) *Result {
	start := startOfWeek(now, loc).AddDate(0, 0, -7)
	end := start.Add(7 * 24 * time.Hour)
	return &Result{
		Window:      TimeWindow{From: &start, To: &end},
		Confidence:  "high",
		MatchedText: matched,
	}
}

func lastMonthResult(matched string, now time.Time, loc *time.Location) *Result {
	start := time.Date(now.Year(), now.Month()-1, 1, 0, 0, 0, 0, loc)
	end := start.AddDate(0, 1, 0)
	return &Result{
		Window:      TimeWindow{From: &start, To: &end},
		Confidence:  "high",
		MatchedText: matched,
	}
}

func lastYearResult(matched string, now time.Time, loc *time.Location) *Result {
	start := time.Date(now.Year()-1, 1, 1, 0, 0, 0, 0, loc)
	end := start.AddDate(1, 0, 0)
	return &Result{
		Window:      TimeWindow{From: &start, To: &end},
		Confidence:  "high",
		MatchedText: matched,
	}
}

func nDurationResult(matched, numStr string, now time.Time, hoursPerUnit int, confidence string) *Result {
	n := 0
	for _, r := range numStr {
		n = n*10 + int(r-'0')
	}
	if n <= 0 {
		return nil
	}
	from := now.Add(-time.Duration(n) * time.Duration(hoursPerUnit) * time.Hour)
	return &Result{
		Window:      TimeWindow{From: &from},
		Confidence:  confidence,
		MatchedText: matched,
	}
}

// startOfWeek returns midnight (UTC) of the Monday of the week containing t.
func startOfWeek(t time.Time, loc *time.Location) time.Time {
	t = t.In(loc)
	weekday := t.Weekday()
	daysSinceMonday := int(weekday) - 1
	if weekday == time.Sunday {
		daysSinceMonday = 6
	}
	start := time.Date(t.Year(), t.Month(), t.Day()-daysSinceMonday, 0, 0, 0, 0, loc)
	return start.UTC()
}
