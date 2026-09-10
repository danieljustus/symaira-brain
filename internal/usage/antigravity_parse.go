package usage

import (
	"encoding/json"
	"math"
	"strings"
	"time"
)

func antigravitySnapshot(method string, data []byte, providerID, source string) (*UsageSnapshot, error) {
	switch method {
	case "RetrieveUserQuotaSummary":
		return antigravityQuotaSummarySnapshot(data, providerID, source)
	case "GetUserStatus":
		return antigravityUserStatusSnapshot(data, providerID, source)
	default:
		return antigravityModelConfigsSnapshot(data, providerID, source)
	}
}

// antigravityQuotaSummarySnapshot parses RetrieveUserQuotaSummary: quota
// groups with named buckets (e.g. "Gemini Models" / "Claude and GPT
// models", each with weekly and five-hour buckets). Each bucket's
// remainingFraction maps to a percent meter.
func antigravityQuotaSummarySnapshot(data []byte, providerID, source string) (*UsageSnapshot, error) {
	var payload antigravityQuotaSummaryEnvelope
	if err := json.Unmarshal(data, &payload); err != nil {
		return nil, &antigravityError{kind: "parse_failed", detail: "quota summary is not JSON"}
	}
	if !payload.Code.isOK() {
		return nil, &antigravityError{kind: "parse_failed", detail: "quota summary rejected"}
	}
	summary := payload.Response
	if summary == nil {
		summary = payload.Summary
	}
	if summary == nil && len(payload.Groups) > 0 {
		summary = &antigravityQuotaSummaryPayload{Groups: payload.Groups}
	}
	if summary == nil || len(summary.Groups) == 0 {
		return nil, &antigravityError{kind: "parse_failed", detail: "missing quota groups"}
	}

	var meters []UsageMeter
	for _, group := range summary.Groups {
		for _, bucket := range group.Buckets {
			if bucket.RemainingFraction == nil || *bucket.RemainingFraction < 0 || *bucket.RemainingFraction > 1 {
				continue
			}
			label := strings.TrimSpace(bucket.displayLabel())
			if label == "" {
				label = group.DisplayName
			}
			var resetsAt *time.Time
			if bucket.ResetTime != nil {
				resetsAt = antigravityParseResetTime(*bucket.ResetTime)
			}
			meters = append(meters, UsageMeter{
				Label:    group.DisplayName + " — " + label,
				Used:     strPtr(formatAmount(antigravityRoundPercent(1 - *bucket.RemainingFraction))),
				Limit:    strPtr("100"),
				Unit:     "%",
				ResetsAt: resetsAt,
			})
		}
	}
	if len(meters) == 0 {
		return nil, &antigravityError{kind: "parse_failed", detail: "quota summary has no usable buckets"}
	}
	return &UsageSnapshot{ProviderID: providerID, Meters: meters, FetchedAt: time.Now().UTC(), Source: source}, nil
}

// antigravityUserStatusSnapshot parses GetUserStatus: plan plus per-model
// quotaInfo buckets.
func antigravityUserStatusSnapshot(data []byte, providerID, source string) (*UsageSnapshot, error) {
	var payload antigravityUserStatusResponse
	if err := json.Unmarshal(data, &payload); err != nil {
		return nil, &antigravityError{kind: "parse_failed", detail: "user status is not JSON"}
	}
	if !payload.Code.isOK() {
		return nil, &antigravityError{kind: "parse_failed", detail: "user status rejected"}
	}
	var configs []antigravityModelConfig
	if payload.UserStatus != nil && payload.UserStatus.CascadeModelConfigData != nil {
		configs = payload.UserStatus.CascadeModelConfigData.ClientModelConfigs
	}
	meters := antigravityModelConfigMeters(configs)
	if len(meters) == 0 {
		return nil, &antigravityError{kind: "parse_failed", detail: "user status has no quota buckets"}
	}
	return &UsageSnapshot{ProviderID: providerID, Meters: meters, FetchedAt: time.Now().UTC(), Source: source}, nil
}

// antigravityModelConfigsSnapshot parses GetCommandModelConfigs: per-model
// quotaInfo buckets.
func antigravityModelConfigsSnapshot(data []byte, providerID, source string) (*UsageSnapshot, error) {
	var payload antigravityModelConfigResponse
	if err := json.Unmarshal(data, &payload); err != nil {
		return nil, &antigravityError{kind: "parse_failed", detail: "model configs are not JSON"}
	}
	if !payload.Code.isOK() {
		return nil, &antigravityError{kind: "parse_failed", detail: "model configs rejected"}
	}
	meters := antigravityModelConfigMeters(payload.ClientModelConfigs)
	if len(meters) == 0 {
		return nil, &antigravityError{kind: "parse_failed", detail: "model configs have no quota buckets"}
	}
	return &UsageSnapshot{ProviderID: providerID, Meters: meters, FetchedAt: time.Now().UTC(), Source: source}, nil
}

func antigravityModelConfigMeters(configs []antigravityModelConfig) []UsageMeter {
	var meters []UsageMeter
	for _, config := range configs {
		if config.QuotaInfo == nil || config.QuotaInfo.RemainingFraction == nil {
			continue
		}
		remaining := *config.QuotaInfo.RemainingFraction
		if remaining < 0 || remaining > 1 {
			continue
		}
		label := strings.TrimSpace(config.displayLabel())
		if label == "" {
			label = config.ModelOrAlias.Model
		}
		var resetsAt *time.Time
		if config.QuotaInfo.ResetTime != nil {
			resetsAt = antigravityParseResetTime(*config.QuotaInfo.ResetTime)
		}
		meters = append(meters, UsageMeter{
			Label:    label,
			Used:     strPtr(formatAmount(antigravityRoundPercent(1 - remaining))),
			Limit:    strPtr("100"),
			Unit:     "%",
			ResetsAt: resetsAt,
		})
	}
	return meters
}

// antigravityRoundPercent converts a 0..1 "used fraction" to a whole
// percent — the raw fraction math carries binary floating-point noise
// (58.000...1), matching the Swift original's explicit .rounded().
func antigravityRoundPercent(usedFraction float64) float64 {
	return math.Round(usedFraction * 100)
}

// antigravityParseResetTime parses Antigravity reset timestamps: ISO8601
// with optional fractional seconds, falling back to plain ISO8601.
func antigravityParseResetTime(value string) *time.Time {
	if t, err := time.Parse(time.RFC3339Nano, value); err == nil {
		return &t
	}
	if t, err := time.Parse(time.RFC3339, value); err == nil {
		return &t
	}
	return nil
}

// MARK: - Response models

// antigravityCode arrives either as an integer (0 = ok) or a string
// ("ok").
type antigravityCode struct {
	intValue    *int
	stringValue *string
}

func (c *antigravityCode) UnmarshalJSON(data []byte) error {
	var i int
	if err := json.Unmarshal(data, &i); err == nil {
		c.intValue = &i
		return nil
	}
	var s string
	if err := json.Unmarshal(data, &s); err == nil {
		c.stringValue = &s
		return nil
	}
	empty := ""
	c.stringValue = &empty
	return nil
}

func (c antigravityCode) isOK() bool {
	if c.intValue != nil {
		return *c.intValue == 0
	}
	if c.stringValue != nil {
		lower := strings.ToLower(*c.stringValue)
		return lower == "ok" || lower == "success" || *c.stringValue == "0"
	}
	return false
}

type antigravityQuotaSummaryEnvelope struct {
	Code     antigravityCode                 `json:"code"`
	Message  *string                         `json:"message"`
	Response *antigravityQuotaSummaryPayload `json:"response"`
	Summary  *antigravityQuotaSummaryPayload `json:"summary"`
	Groups   []antigravityQuotaGroup         `json:"groups"`
}

type antigravityQuotaSummaryPayload struct {
	Description *string                 `json:"description"`
	Groups      []antigravityQuotaGroup `json:"groups"`
}

type antigravityQuotaGroup struct {
	DisplayName string                   `json:"displayName"`
	Description *string                  `json:"description"`
	Buckets     []antigravityQuotaBucket `json:"buckets"`
}

type antigravityQuotaBucket struct {
	BucketID          string   `json:"bucketId"`
	DisplayName       *string  `json:"displayName"`
	RemainingFraction *float64 `json:"remainingFraction"`
	ResetTime         *string  `json:"resetTime"`
	Description       *string  `json:"description"`
	Disabled          *bool    `json:"disabled"`
}

func (b antigravityQuotaBucket) displayLabel() string {
	if b.DisplayName == nil {
		return ""
	}
	return *b.DisplayName
}

type antigravityUserStatusResponse struct {
	Code       antigravityCode        `json:"code"`
	Message    *string                `json:"message"`
	UserStatus *antigravityUserStatus `json:"userStatus"`
}

type antigravityUserStatus struct {
	Email                  *string                     `json:"email"`
	CascadeModelConfigData *antigravityModelConfigData `json:"cascadeModelConfigData"`
}

type antigravityModelConfigData struct {
	ClientModelConfigs []antigravityModelConfig `json:"clientModelConfigs"`
}

type antigravityModelConfigResponse struct {
	Code               antigravityCode          `json:"code"`
	Message            *string                  `json:"message"`
	ClientModelConfigs []antigravityModelConfig `json:"clientModelConfigs"`
}

type antigravityModelConfig struct {
	Label        *string               `json:"label"`
	ModelOrAlias antigravityModelAlias `json:"modelOrAlias"`
	QuotaInfo    *antigravityQuotaInfo `json:"quotaInfo"`
}

func (c antigravityModelConfig) displayLabel() string {
	if c.Label == nil {
		return ""
	}
	return *c.Label
}

type antigravityModelAlias struct {
	Model string `json:"model"`
}

type antigravityQuotaInfo struct {
	RemainingFraction *float64 `json:"remainingFraction"`
	ResetTime         *string  `json:"resetTime"`
}
