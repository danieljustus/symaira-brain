package usage

import (
	"context"
	"net/http"
	"sync"
	"time"
)

// providerFetchTimeout bounds a single provider's fetch (its Strategies
// chain, including any HTTP calls) independent of the overall report
// deadline (issue #429): a provider that stalls no longer starves the
// providers behind it. Also used as the default http.Client timeout in
// AllProviders, so a fetch stays bounded even if a future caller invokes a
// Strategy outside BuildReport's per-provider context.
//
// A var, not a const, solely so tests can shrink it and exercise the
// timeout path without a multi-second sleep; BuildReport and AllProviders
// are the only production readers and both run it synchronously with
// respect to any caller, so a test that restores the original value after
// BuildReport returns never races a concurrent read.
var providerFetchTimeout = 8 * time.Second

// AllProviders returns every registered provider (issue #290: all 10 of
// symaira-cockpit's AI-usage providers — OpenCodeWebUsageStrategy.swift is
// a strategy within the OpenCode provider, not an 11th provider).
//
// Scope note: every provider's credential resolution here is portable (env
// vars and plain file reads), with one exception — Claude also reads the
// macOS login keychain, because that is the only place Claude Code stores
// its token on a Mac; it goes through a build-tagged helper that is a no-op
// elsewhere. Two providers (Cursor, OpenCode) drop a second,
// SQLite-database-backed local-history strategy that would need a new
// dependency decision (CGO vs. a pure-Go/WASM SQLite reader) to port
// cross-platform. Each provider's doc comment states exactly which of the
// Swift original's strategies made the cut and why.
//
// client is nil in every current caller: a nil client here is given an
// explicit Timeout (issue #429) instead of falling through to each
// provider's own http.DefaultClient fallback, which has no timeout of its
// own.
func AllProviders(client *http.Client) []Provider {
	if client == nil {
		client = &http.Client{Timeout: providerFetchTimeout}
	}
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
//
// Configured providers are fanned out concurrently (issue #429), each
// bounded by its own providerFetchTimeout derived from ctx, so one stalled
// provider can no longer consume the entire caller-supplied budget (e.g.
// the 30s deadline cmd_usage.go and the gateway's get_ai_usage tool wrap
// this call in) and block every provider behind it. Results are collected
// into a slice pre-sized and indexed by provider position, so report order
// matches the input order regardless of which goroutine finishes first.
func BuildReport(ctx context.Context, providers []Provider) Report {
	report := Report{
		SchemaVersion: ReportSchemaVersion,
		Providers:     make([]ProviderUsage, len(providers)),
	}

	var wg sync.WaitGroup
	for i, p := range providers {
		pu := ProviderUsage{
			ID:          p.ID(),
			DisplayName: p.DisplayName(),
			Configured:  p.IsConfigured(),
			AuthStatus:  p.AuthStatus(),
		}
		if !pu.Configured {
			report.Providers[i] = pu
			continue
		}

		wg.Add(1)
		go func(i int, p Provider, pu ProviderUsage) {
			defer wg.Done()
			fetchCtx, cancel := context.WithTimeout(ctx, providerFetchTimeout)
			defer cancel()

			snap, err := RunStrategyChain(fetchCtx, p.Strategies())
			switch {
			case err == nil:
				pu.Snapshot = snap
			case fetchCtx.Err() == context.DeadlineExceeded:
				// The per-provider budget (not the caller's outer context)
				// expired: report a dedicated timeout status rather than
				// whatever context-cancellation string the strategy chain
				// happened to surface.
				pu.Error = (&TimeoutError{ProviderID: p.ID(), Timeout: providerFetchTimeout}).Error()
			default:
				pu.Error = err.Error()
			}
			report.Providers[i] = pu
		}(i, p, pu)
	}
	wg.Wait()

	return report
}
