package usage

import (
	"context"
	"net/http"
)

// AllProviders returns every registered provider (issue #290: all 10 of
// symaira-cockpit's AI-usage providers — OpenCodeWebUsageStrategy.swift is
// a strategy within the OpenCode provider, not an 11th provider).
//
// Scope note: every provider's credential resolution here is portable (env
// vars and plain file reads) — the Swift originals' macOS-Keychain
// fallbacks are not ported, and two providers (Cursor, OpenCode) drop a
// second, SQLite-database-backed local-history strategy that would need a
// new dependency decision (CGO vs. a pure-Go/WASM SQLite reader) to port
// cross-platform. Each provider's doc comment states exactly which of the
// Swift original's strategies made the cut and why.
func AllProviders(client *http.Client) []Provider {
	return []Provider{
		NewClaudeProvider(client),
		NewCodexProvider(client),
		NewCopilotProvider(client),
		NewCursorProvider(client),
		NewKimiProvider(client),
		NewMoonshotProvider(client),
		NewNousPortalProvider(client),
		NewOpenCodeProvider(client),
		NewOpenRouterProvider(client),
		NewAntigravityProvider(),
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
