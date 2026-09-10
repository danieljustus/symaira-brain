package usage

import (
	"encoding/json"
	"regexp"
	"strconv"
	"time"
)

func openCodeParseSubscription(text string, now time.Time) (*UsageSnapshot, error) {
	if snap := openCodeParseSubscriptionJSON(text, now); snap != nil {
		return snap, nil
	}
	return openCodeParseSubscriptionRegex(text, now)
}

type openCodeWindow struct {
	percent    float64
	resetInSec *float64
}

func openCodeParseSubscriptionJSON(text string, now time.Time) *UsageSnapshot {
	var object any
	if err := json.Unmarshal([]byte(text), &object); err != nil {
		return nil
	}
	var windows []openCodeWindow
	openCodeCollectUsageWindows(object, &windows)
	if len(windows) == 0 {
		return nil
	}
	rolling := windows[0]
	var weekly *openCodeWindow
	if len(windows) > 1 {
		weekly = &windows[1]
	}
	return openCodeMakeSnapshot(rolling, weekly, now)
}

// openCodeCollectUsageWindows recursively collects dicts that carry a
// usage-percent field plus a reset field. Named windows (rollingUsage,
// weeklyUsage, ...) are traversed in a fixed order because Go map
// iteration order is random; the first collected window is the rolling
// one.
func openCodeCollectUsageWindows(object any, windows *[]openCodeWindow) {
	dict, ok := object.(map[string]any)
	if !ok {
		if array, ok := object.([]any); ok {
			for _, value := range array {
				openCodeCollectUsageWindows(value, windows)
			}
		}
		return
	}
	var named []any
	for _, key := range []string{"rollingUsage", "weeklyUsage", "usage", "billing", "data", "result"} {
		if value, ok := dict[key]; ok {
			named = append(named, value)
		}
	}
	if len(named) > 0 {
		for _, value := range named {
			openCodeCollectUsageWindows(value, windows)
		}
		return
	}
	if percent, ok := openCodeUsagePercent(dict); ok {
		*windows = append(*windows, openCodeWindow{percent: percent, resetInSec: openCodeResetSeconds(dict)})
	}
	for _, value := range dict {
		openCodeCollectUsageWindows(value, windows)
	}
}

func openCodeUsagePercent(dict map[string]any) (float64, bool) {
	for _, key := range []string{"usagePercent", "usedPercent", "percentUsed", "percent", "usage_percent", "utilization", "usage"} {
		if value, ok := dict[key]; ok {
			if f, ok := value.(float64); ok {
				return f, true
			}
		}
	}
	return 0, false
}

func openCodeResetSeconds(dict map[string]any) *float64 {
	for _, key := range []string{"resetInSec", "resetInSeconds", "reset_sec", "resetsInSec", "resetIn", "resetSec"} {
		if value, ok := dict[key]; ok {
			if f, ok := value.(float64); ok {
				return &f
			}
		}
	}
	return nil
}

var (
	openCodeRollingPercentPattern = regexp.MustCompile(`rollingUsage[^}]*?usagePercent\s*:\s*([0-9]+(?:\.[0-9]+)?)`)
	openCodePercentPattern        = regexp.MustCompile(`(?:usagePercent|usedPercent|percentUsed|percent)\s*:\s*([0-9]+(?:\.[0-9]+)?)`)
	openCodeRollingResetPattern   = regexp.MustCompile(`rollingUsage[^}]*?resetInSec\s*:\s*([0-9]+)`)
	openCodeResetPattern          = regexp.MustCompile(`(?:resetInSec|resetSeconds|resetIn)\s*:\s*([0-9]+)`)
	openCodeWeeklyPercentPattern  = regexp.MustCompile(`weeklyUsage[^}]*?usagePercent\s*:\s*([0-9]+(?:\.[0-9]+)?)`)
	openCodeWeeklyResetPattern    = regexp.MustCompile(`weeklyUsage[^}]*?resetInSec\s*:\s*([0-9]+)`)
)

func openCodeParseSubscriptionRegex(text string, now time.Time) (*UsageSnapshot, error) {
	rollingPercentStr := openCodeExtract(openCodeRollingPercentPattern, text)
	if rollingPercentStr == "" {
		rollingPercentStr = openCodeExtract(openCodePercentPattern, text)
	}
	rollingResetStr := openCodeExtract(openCodeRollingResetPattern, text)
	if rollingResetStr == "" {
		rollingResetStr = openCodeExtract(openCodeResetPattern, text)
	}
	if rollingPercentStr == "" || rollingResetStr == "" {
		return nil, &openCodeError{kind: "parse_failed", detail: "missing usage fields"}
	}
	rollingPercent, err := strconv.ParseFloat(rollingPercentStr, 64)
	if err != nil {
		return nil, &openCodeError{kind: "parse_failed", detail: "missing usage fields"}
	}
	rollingReset, err := strconv.ParseFloat(rollingResetStr, 64)
	if err != nil {
		return nil, &openCodeError{kind: "parse_failed", detail: "missing usage fields"}
	}
	rolling := openCodeWindow{percent: rollingPercent, resetInSec: &rollingReset}

	var weekly *openCodeWindow
	if weeklyPercentStr := openCodeExtract(openCodeWeeklyPercentPattern, text); weeklyPercentStr != "" {
		if weeklyPercent, err := strconv.ParseFloat(weeklyPercentStr, 64); err == nil {
			w := openCodeWindow{percent: weeklyPercent}
			if weeklyResetStr := openCodeExtract(openCodeWeeklyResetPattern, text); weeklyResetStr != "" {
				if weeklyReset, err := strconv.ParseFloat(weeklyResetStr, 64); err == nil {
					w.resetInSec = &weeklyReset
				}
			}
			weekly = &w
		}
	}
	return openCodeMakeSnapshot(rolling, weekly, now), nil
}

func openCodeExtract(pattern *regexp.Regexp, text string) string {
	match := pattern.FindStringSubmatch(text)
	if len(match) < 2 {
		return ""
	}
	return match[1]
}

// openCodeClampPercent clamps an already-percent value (0..100) to that
// range — unlike clampPercent (cursor.go), which converts a 0..1 fraction.
func openCodeClampPercent(percent float64) float64 {
	if percent < 0 {
		return 0
	}
	if percent > 100 {
		return 100
	}
	return percent
}

func openCodeMakeSnapshot(rolling openCodeWindow, weekly *openCodeWindow, now time.Time) *UsageSnapshot {
	var meters []UsageMeter
	var rollingReset *time.Time
	if rolling.resetInSec != nil {
		t := now.Add(time.Duration(*rolling.resetInSec * float64(time.Second)))
		rollingReset = &t
	}
	meters = append(meters, UsageMeter{
		Label:    "5h window",
		Used:     strPtr(formatAmount(openCodeClampPercent(rolling.percent))),
		Limit:    strPtr("100"),
		Unit:     "%",
		ResetsAt: rollingReset,
	})
	if weekly != nil {
		var weeklyReset *time.Time
		if weekly.resetInSec != nil {
			t := now.Add(time.Duration(*weekly.resetInSec * float64(time.Second)))
			weeklyReset = &t
		}
		meters = append(meters, UsageMeter{
			Label:    "This week",
			Used:     strPtr(formatAmount(openCodeClampPercent(weekly.percent))),
			Limit:    strPtr("100"),
			Unit:     "%",
			ResetsAt: weeklyReset,
		})
	}
	return &UsageSnapshot{
		ProviderID: openCodeProviderID,
		Meters:     meters,
		FetchedAt:  now,
		Source:     "web",
	}
}
