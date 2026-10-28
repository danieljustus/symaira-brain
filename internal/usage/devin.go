package usage

import (
	"bytes"
	"context"
	"encoding/json"
	"math"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/BurntSushi/toml"
)

// DevinProvider uses the Codeium-compatible Devin credential, not a devin.ai
// organization API key. Never forward it to a host named by a credential file.
type DevinProvider struct {
	key, source string
	err         error
	client      *http.Client
}

func NewDevinProvider(client *http.Client) *DevinProvider {
	key, source, err := resolveFileCredential("DEVIN_API_KEY", func() string {
		home, e := os.UserHomeDir()
		if e != nil {
			return ""
		}
		return devinFileKey(filepath.Join(home, ".local", "share", "devin", "credentials.toml"))
	})
	return &DevinProvider{key: key, source: source, err: err, client: client}
}
func devinFileKey(path string) string {
	info, err := os.Lstat(path)
	if err != nil || !info.Mode().IsRegular() {
		return ""
	}
	data, err := readCredentialFile(path)
	if err != nil {
		return ""
	}
	var doc struct {
		Key string `toml:"windsurf_api_key"`
	}
	if _, err := toml.Decode(string(data), &doc); err != nil {
		return ""
	}
	key := strings.TrimSpace(doc.Key)
	if strings.ContainsAny(key, "\r\n\x00") {
		return ""
	}
	return key
}
func (p *DevinProvider) ID() string          { return "devin" }
func (p *DevinProvider) DisplayName() string { return "Devin" }
func (p *DevinProvider) IsConfigured() bool  { return p.err == nil && p.key != "" }
func (p *DevinProvider) AuthStatus() AuthStatus {
	if p.err != nil {
		return AuthStatus{Status: "missing", Detail: "credential resolution failed", Source: p.source}
	}
	if !p.IsConfigured() {
		return AuthStatus{Status: "missing", Detail: "no Devin credential (DEVIN_API_KEY or Devin CLI credentials.toml)"}
	}
	return AuthStatus{Status: "available", Detail: "Devin credential configured", Source: p.source}
}
func (p *DevinProvider) Strategies() []Strategy {
	if !p.IsConfigured() {
		return nil
	}
	return []Strategy{&devinStrategy{p}}
}

type devinStrategy struct{ p *DevinProvider }

func (s *devinStrategy) Source() string { return "api" }
func (s *devinStrategy) Fetch(ctx context.Context) (*UsageSnapshot, error) {
	body, err := json.Marshal(map[string]any{"metadata": map[string]string{"apiKey": s.p.key, "ideName": "devin", "ideVersion": "1.108.2", "extensionName": "devin", "extensionVersion": "1.108.2", "locale": "en"}})
	if err != nil {
		return nil, &HTTPError{Kind: "invalid_response"}
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, "https://server.codeium.com/exa.seat_management_pb.SeatManagementService/GetUserStatus", bytes.NewReader(body))
	if err != nil {
		return nil, &HTTPError{Kind: "invalid_response"}
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Connect-Protocol-Version", "1")
	root, err := FetchJSON[map[string]json.RawMessage](ctx, req, "devin", s.p.client)
	if err != nil {
		return nil, quotaSafeError(err)
	}
	return parseDevinQuota(root)
}
func parseDevinQuota(root map[string]json.RawMessage) (*UsageSnapshot, error) {
	bad := func() (*UsageSnapshot, error) { return nil, &HTTPError{Kind: "unparseable"} }
	var user struct {
		PlanStatus map[string]json.RawMessage `json:"planStatus"`
	}
	if json.Unmarshal(root["userStatus"], &user) != nil || user.PlanStatus == nil {
		return bad()
	}
	ps := user.PlanStatus
	var plan struct {
		Hide bool `json:"hideDailyQuota"`
	}
	if raw, ok := ps["planInfo"]; ok && json.Unmarshal(raw, &plan) != nil {
		return bad()
	}
	daily, dailyOK := quotaNumber(ps["dailyQuotaRemainingPercent"])
	weekly, weeklyOK := quotaNumber(ps["weeklyQuotaRemainingPercent"])
	if _, present := ps["weeklyQuotaRemainingPercent"]; present && !weeklyOK {
		return bad()
	}
	dailyReset, err := quotaReset(ps["dailyQuotaResetAtUnix"], false)
	if err != nil {
		return bad()
	}
	weeklyReset, err := quotaReset(ps["weeklyQuotaResetAtUnix"], false)
	if err != nil {
		return bad()
	}
	snap := &UsageSnapshot{ProviderID: "devin", Meters: []UsageMeter{}, FetchedAt: time.Now().UTC(), Source: "api"}
	add := func(label string, remaining float64, reset *time.Time) {
		snap.Meters = append(snap.Meters, UsageMeter{Label: label, Used: strPtr(formatAmount(math.Max(0, math.Min(100, 100-remaining)))), Limit: strPtr("100"), Unit: "%", ResetsAt: reset})
	}
	if !plan.Hide && dailyOK {
		add("Daily quota", daily, dailyReset)
	}
	if !weeklyOK && weeklyReset != nil {
		weekly, weeklyOK = 0, true
	}
	if weeklyOK {
		add("Weekly quota", weekly, weeklyReset)
	} else if plan.Hide && dailyOK {
		add("Weekly quota", daily, nil)
	}
	if balance, ok := quotaNumber(ps["overageBalanceMicros"]); ok {
		snap.Balance = strPtr(formatAmount(math.Max(0, balance) / 1e6))
		snap.Currency = strPtr("USD")
		snap.Meters = append(snap.Meters, UsageMeter{Label: "Extra usage balance", Used: snap.Balance, Unit: "USD"})
	}
	if len(snap.Meters) == 0 {
		return bad()
	}
	return snap, nil
}
