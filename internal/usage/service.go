package usage

import (
	"context"
	"net/http"
)

// AllProviders returns every registered provider. Progress note (issue
// #290): only the providers whose credential model needed no per-provider
// architecture decision (plain API key or a portable auth-store file, no
// macOS Keychain / CLI-OAuth-file complexity) are ported so far. The
// remaining providers (Claude, Kimi, OpenCode, Cursor, Antigravity,
// Copilot, Codex) are tracked as follow-up work on #290.
func AllProviders(client *http.Client) []Provider {
	return []Provider{
		NewOpenRouterProvider(client),
		NewMoonshotProvider(client),
		NewNousPortalProvider(client),
	}
}

// BuildReport fetches usage for every configured provider and assembles a
// Report matching the schema_version 1 contract (issue #289). Unconfigured
// providers are reported with Configured=false and no fetch attempt — a
// provider with no credential has nothing to fetch.
func BuildReport(ctx context.Context, providers []Provider) Report {
	report := Report{SchemaVersion: ReportSchemaVersion}
	for _, p := range providers {
		pu := ProviderUsage{
			ID:          p.ID(),
			DisplayName: p.DisplayName(),
			Configured:  p.IsConfigured(),
			AuthStatus:  p.AuthStatus(),
		}
		if pu.Configured {
			snap, err := RunStrategyChain(ctx, p.Strategies())
			if err != nil {
				pu.Error = err.Error()
			} else {
				pu.Snapshot = snap
			}
		}
		report.Providers = append(report.Providers, pu)
	}
	return report
}
