package usage

import (
	"encoding/json"
	"testing"
	"time"
)

func TestReportSchemaVersionIsStable(t *testing.T) {
	if ReportSchemaVersion != 1 {
		t.Fatalf("ReportSchemaVersion = %d, want 1 — bumping this is a breaking-contract change for cockpit#22", ReportSchemaVersion)
	}
}

func TestReportMarshalsSnakeCaseKeys(t *testing.T) {
	used := "1200"
	limit := "5000"
	balance := "42.50"
	currency := "USD"
	resetsAt := time.Date(2026, 9, 1, 0, 0, 0, 0, time.UTC)
	fetchedAt := time.Date(2026, 8, 24, 12, 0, 0, 0, time.UTC)

	report := Report{
		SchemaVersion: ReportSchemaVersion,
		Providers: []ProviderUsage{
			{
				ID:          "claude",
				DisplayName: "Claude",
				Configured:  true,
				AuthStatus: AuthStatus{
					Status: "available",
					Detail: "via Claude Code OAuth token",
					Source: "oauth",
				},
				Snapshot: &UsageSnapshot{
					ProviderID: "claude",
					Meters: []UsageMeter{
						{Label: "this session", Used: &used, Limit: &limit, Unit: "tokens", ResetsAt: &resetsAt},
					},
					Balance:   &balance,
					Currency:  &currency,
					FetchedAt: fetchedAt,
					Source:    "oauth",
				},
			},
			{
				ID:          "openrouter",
				DisplayName: "OpenRouter",
				Configured:  false,
				AuthStatus:  AuthStatus{Status: "missing", Detail: "no API key configured"},
			},
		},
	}

	data, err := json.Marshal(report)
	if err != nil {
		t.Fatalf("Marshal(): %v", err)
	}

	var decoded map[string]any
	if err := json.Unmarshal(data, &decoded); err != nil {
		t.Fatalf("Unmarshal(): %v", err)
	}

	if _, ok := decoded["schema_version"]; !ok {
		t.Fatalf("missing schema_version key, got: %s", data)
	}
	providers, ok := decoded["providers"].([]any)
	if !ok || len(providers) != 2 {
		t.Fatalf("providers = %v, want 2 entries", decoded["providers"])
	}

	configured := providers[0].(map[string]any)
	for _, key := range []string{"id", "display_name", "configured", "auth_status", "snapshot"} {
		if _, ok := configured[key]; !ok {
			t.Errorf("configured provider missing key %q, got: %s", key, data)
		}
	}
	snapshot := configured["snapshot"].(map[string]any)
	for _, key := range []string{"provider_id", "meters", "balance", "currency", "fetched_at", "source"} {
		if _, ok := snapshot[key]; !ok {
			t.Errorf("snapshot missing key %q, got: %s", key, data)
		}
	}
	meter := snapshot["meters"].([]any)[0].(map[string]any)
	for _, key := range []string{"label", "used", "limit", "unit", "resets_at"} {
		if _, ok := meter[key]; !ok {
			t.Errorf("meter missing key %q, got: %s", key, data)
		}
	}

	unconfigured := providers[1].(map[string]any)
	if _, ok := unconfigured["snapshot"]; ok {
		t.Errorf("unconfigured provider should omit snapshot entirely, got: %s", data)
	}
	if _, ok := unconfigured["error"]; ok {
		t.Errorf("unconfigured provider without a fetch attempt should omit error, got: %s", data)
	}
}

func TestReportRoundTrips(t *testing.T) {
	used := "10"
	original := Report{
		SchemaVersion: ReportSchemaVersion,
		Providers: []ProviderUsage{
			{
				ID:          "codex",
				DisplayName: "Codex",
				Configured:  true,
				AuthStatus:  AuthStatus{Status: "expired", Detail: "re-auth needed", Source: "cli"},
				Error:       "token expired",
			},
			{
				ID:          "cursor",
				DisplayName: "Cursor",
				Configured:  true,
				AuthStatus:  AuthStatus{Status: "available", Source: "keyring"},
				Snapshot: &UsageSnapshot{
					ProviderID: "cursor",
					Meters:     []UsageMeter{{Label: "requests", Used: &used, Unit: "requests"}},
					FetchedAt:  time.Date(2026, 8, 24, 0, 0, 0, 0, time.UTC),
					Source:     "api",
				},
			},
		},
	}

	data, err := json.Marshal(original)
	if err != nil {
		t.Fatalf("Marshal(): %v", err)
	}

	var decoded Report
	if err := json.Unmarshal(data, &decoded); err != nil {
		t.Fatalf("Unmarshal(): %v", err)
	}

	if decoded.SchemaVersion != original.SchemaVersion {
		t.Errorf("SchemaVersion = %d, want %d", decoded.SchemaVersion, original.SchemaVersion)
	}
	if len(decoded.Providers) != len(original.Providers) {
		t.Fatalf("Providers length = %d, want %d", len(decoded.Providers), len(original.Providers))
	}
	if decoded.Providers[0].Error != "token expired" {
		t.Errorf("Providers[0].Error = %q, want %q", decoded.Providers[0].Error, "token expired")
	}
	if decoded.Providers[1].Snapshot == nil || decoded.Providers[1].Snapshot.Meters[0].Label != "requests" {
		t.Errorf("Providers[1].Snapshot round-trip mismatch: %+v", decoded.Providers[1].Snapshot)
	}
}
