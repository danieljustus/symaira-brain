package usage

import (
	"context"
	"encoding/json"
	"math"
	"net/http"
	"strconv"
	"time"
)

// ZAIProvider reads coding-plan quotas, not pay-as-you-go token spend.
// Only ZAI_API_KEY is supported; secret references use Brain's shared resolver.
type ZAIProvider struct {
	key, source string
	err         error
	client      *http.Client
}

func NewZAIProvider(client *http.Client) *ZAIProvider {
	key, source, err := resolveEnv("ZAI_API_KEY")
	return &ZAIProvider{key: key, source: source, err: err, client: client}
}
func (p *ZAIProvider) ID() string          { return "zai" }
func (p *ZAIProvider) DisplayName() string { return "Z.ai" }
func (p *ZAIProvider) IsConfigured() bool  { return p.err == nil && p.key != "" }
func (p *ZAIProvider) AuthStatus() AuthStatus {
	if p.err != nil {
		return AuthStatus{Status: "missing", Detail: "credential resolution failed", Source: p.source}
	}
	if !p.IsConfigured() {
		return AuthStatus{Status: "missing", Detail: "no API key configured (ZAI_API_KEY)"}
	}
	return AuthStatus{Status: "available", Detail: "API key configured (ZAI_API_KEY)", Source: p.source}
}
func (p *ZAIProvider) Strategies() []Strategy {
	if !p.IsConfigured() {
		return nil
	}
	return []Strategy{&zaiStrategy{p}}
}

type zaiStrategy struct{ p *ZAIProvider }

func (s *zaiStrategy) Source() string { return "api" }
func (s *zaiStrategy) Fetch(ctx context.Context) (*UsageSnapshot, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, "https://api.z.ai/api/monitor/usage/quota/limit", nil)
	if err != nil {
		return nil, &HTTPError{Kind: "invalid_response"}
	}
	req.Header.Set("Authorization", "Bearer "+s.p.key)
	root, err := FetchJSON[map[string]json.RawMessage](ctx, req, "zai", s.p.client)
	// Do not expose transport diagnostics: they may echo a credential header.
	if err != nil {
		return nil, quotaSafeError(err)
	}
	return parseZAIQuota(root)
}
func quotaSafeError(err error) error {
	if e, ok := err.(*HTTPError); ok {
		return &HTTPError{Kind: e.Kind, Status: e.Status}
	}
	return err
}
func quotaNumber(raw json.RawMessage) (float64, bool) {
	var v any
	if json.Unmarshal(raw, &v) != nil {
		return 0, false
	}
	n, ok := v.(float64)
	if text, isString := v.(string); isString {
		var err error
		n, err = strconv.ParseFloat(text, 64)
		ok = err == nil
	}
	return n, ok && !math.IsNaN(n) && !math.IsInf(n, 0)
}
func quotaReset(raw json.RawMessage, milliseconds bool) (*time.Time, error) {
	if len(raw) == 0 || string(raw) == "null" {
		return nil, nil
	}
	n, ok := quotaNumber(raw)
	if !ok || n < 0 || n > 253402300799000 {
		return nil, &HTTPError{Kind: "unparseable"}
	}
	if milliseconds {
		t := time.UnixMilli(int64(n)).UTC()
		return &t, nil
	}
	if n > 253402300799 {
		return nil, &HTTPError{Kind: "unparseable"}
	}
	t := time.Unix(int64(n), int64(math.Round((n-math.Floor(n))*1e9))).UTC()
	return &t, nil
}
func parseZAIQuota(root map[string]json.RawMessage) (*UsageSnapshot, error) {
	bad := func() (*UsageSnapshot, error) { return nil, &HTTPError{Kind: "unparseable"} }
	if string(root["success"]) == "false" {
		return nil, &PayloadError{ProviderID: "zai", Detail: "quota unavailable (no active coding plan or provider failure)"}
	}
	if data, ok := root["data"]; ok {
		var container map[string]json.RawMessage
		if json.Unmarshal(data, &container) != nil || container == nil {
			return bad()
		}
		root = container
	}
	var limits []map[string]json.RawMessage
	if raw, ok := root["limits"]; !ok || string(raw) == "null" || json.Unmarshal(raw, &limits) != nil {
		return bad()
	}
	snap := &UsageSnapshot{ProviderID: "zai", Meters: []UsageMeter{}, FetchedAt: time.Now().UTC(), Source: "api"}
	for _, e := range limits {
		if e == nil {
			return bad()
		}
		var kind string
		if json.Unmarshal(e["type"], &kind) != nil {
			_ = json.Unmarshal(e["name"], &kind)
		}
		label, unit := "", "%"
		var used, limit float64
		switch kind {
		case "CREDIT_LIMIT", "TOKENS_LIMIT":
			u, ok := quotaNumber(e["unit"])
			if !ok {
				return bad()
			}
			n, ok := quotaNumber(e["number"])
			if !ok || n <= 0 {
				return bad()
			}
			hours := map[float64]float64{3: 1, 4: 24, 5: 720, 6: 168}[u]
			if hours == 0 {
				continue
			}
			duration := hours * n
			if math.IsInf(duration, 0) || duration < 1.0/3600000 || duration > 2.56e12 {
				return bad()
			}
			label = "Weekly"
			if duration < 24 {
				label = "Session"
			}
			used, ok = quotaNumber(e["percentage"])
			if !ok {
				return bad()
			}
			used = math.Max(0, math.Min(100, used))
			limit = 100
		case "TIME_LIMIT":
			label, unit = "Web Searches", "searches"
			var ok bool
			used, ok = quotaNumber(e["currentValue"])
			if !ok || used < 0 {
				return bad()
			}
			limit, ok = quotaNumber(e["usage"])
			if !ok || limit < 0 {
				return bad()
			}
		default:
			continue
		}
		reset, err := quotaReset(e["nextResetTime"], true)
		if err != nil {
			return nil, err
		}
		snap.Meters = append(snap.Meters, UsageMeter{Label: label, Used: strPtr(formatAmount(used)), Limit: strPtr(formatAmount(limit)), Unit: unit, ResetsAt: reset})
	}
	return snap, nil
}
