package usage

import (
	"encoding/json"
	"strconv"
	"time"
)

func kimiSnapshotFromUsageResponse(data []byte, source string) (*UsageSnapshot, error) {
	var payload kimiUsageResponse
	if err := json.Unmarshal(data, &payload); err != nil {
		return nil, &kimiError{kind: "unparseable"}
	}
	meters := kimiMeters(payload.Usage, "Weekly quota")
	for _, limit := range payload.Limits {
		meters = append(meters, kimiMeters(limit.Detail, kimiWindowLabel(limit.Window))...)
	}
	return &UsageSnapshot{ProviderID: kimiProviderID, Meters: meters, FetchedAt: time.Now().UTC(), Source: source}, nil
}

func kimiSnapshotFromWebResponse(data []byte, source string) (*UsageSnapshot, error) {
	var payload kimiWebUsagesResponse
	if err := json.Unmarshal(data, &payload); err != nil {
		return nil, &kimiError{kind: "unparseable"}
	}
	var meters []UsageMeter
	var coding *kimiWebUsage
	for i := range payload.Usages {
		if payload.Usages[i].Scope == "FEATURE_CODING" {
			coding = &payload.Usages[i]
			break
		}
	}
	if coding == nil && len(payload.Usages) > 0 {
		coding = &payload.Usages[0]
	}
	if coding != nil {
		meters = append(meters, kimiMeters(coding.Detail, "Weekly quota")...)
		for _, limit := range coding.Limits {
			meters = append(meters, kimiMeters(limit.Detail, kimiWindowLabel(limit.Window))...)
		}
	}
	return &UsageSnapshot{ProviderID: kimiProviderID, Meters: meters, FetchedAt: time.Now().UTC(), Source: source}, nil
}

// kimiMeters produces one meter per usage detail (used/limit/reset), or
// none when the payload carries no usable numbers.
func kimiMeters(detail *kimiUsageDetail, label string) []UsageMeter {
	if detail == nil {
		return nil
	}
	used, usedOK := parseOptionalFloat(detail.Used)
	limit, limitOK := parseOptionalFloat(detail.Limit)
	if !usedOK || !limitOK || limit <= 0 {
		return nil
	}
	var resetsAt *time.Time
	if detail.ResetTime != nil {
		resetsAt = kimiParseResetTime(*detail.ResetTime)
	}
	return []UsageMeter{{
		Label:    label,
		Used:     strPtr(formatAmount(used)),
		Limit:    strPtr(formatAmount(limit)),
		Unit:     "requests",
		ResetsAt: resetsAt,
	}}
}

// kimiWindowLabel produces a human label for a rate-limit window, e.g. "5h
// window" for the 300-minute window; falls back to a generic label.
func kimiWindowLabel(window *kimiWindow) string {
	if window == nil || window.Duration == nil || *window.Duration <= 0 {
		return "Rate limit window"
	}
	duration := *window.Duration
	if duration%60 == 0 {
		return strconv.Itoa(duration/60) + "h window"
	}
	return strconv.Itoa(duration) + "min window"
}

// kimiParseResetTime parses Kimi reset timestamps: ISO8601 with optional
// nanosecond fractional seconds, falling back to plain ISO8601.
func kimiParseResetTime(value string) *time.Time {
	if t, err := time.Parse(time.RFC3339Nano, value); err == nil {
		return &t
	}
	if t, err := time.Parse(time.RFC3339, value); err == nil {
		return &t
	}
	return nil
}

// Kimi returns quota numbers as decimal strings ("2048") and reset times
// with nanosecond fractional seconds.
type kimiUsageDetail struct {
	Limit     *string `json:"limit"`
	Used      *string `json:"used"`
	Remaining *string `json:"remaining"`
	ResetTime *string `json:"resetTime"`
}

type kimiWindow struct {
	Duration *int    `json:"duration"`
	TimeUnit *string `json:"timeUnit"`
}

type kimiLimit struct {
	Window *kimiWindow      `json:"window"`
	Detail *kimiUsageDetail `json:"detail"`
}

// kimiUsageResponse — Kimi Code API response (GET /coding/v1/usages).
type kimiUsageResponse struct {
	Usage  *kimiUsageDetail `json:"usage"`
	Limits []kimiLimit      `json:"limits"`
}

// kimiWebUsagesResponse — web billing response (GetUsages).
type kimiWebUsagesResponse struct {
	Usages []kimiWebUsage `json:"usages"`
}

type kimiWebUsage struct {
	Scope  string           `json:"scope"`
	Detail *kimiUsageDetail `json:"detail"`
	Limits []kimiLimit      `json:"limits"`
}
