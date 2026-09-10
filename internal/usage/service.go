package usage

import (
	"context"
	"net/http"
	"sync"
	"time"
)

// providerFetchTimeout bounds a single provider's strategy chain independent
// of the overall report deadline. It is also the default HTTP timeout.
var providerFetchTimeout = 8 * time.Second

// MaxConcurrentProviders is the process-wide fan-out bound for one report.
// Each provider's strategy chain is sequential, so its per-provider bound is
// one in-flight strategy at a time.
const MaxConcurrentProviders = 4

// AllProviders returns every registered provider in stable contract order.
func AllProviders(client *http.Client) []Provider {
	if client == nil {
		client = newProviderHTTPClient()
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

// BuildReport fetches usage for every configured provider. At most
// MaxConcurrentProviders provider chains run at once, each chain has a
// providerFetchTimeout deadline, and results retain contract order.
func BuildReport(ctx context.Context, providers []Provider) Report {
	report := Report{
		SchemaVersion: ReportSchemaVersion,
		Providers:     make([]ProviderUsage, len(providers)),
	}

	var wg sync.WaitGroup
	semaphore := make(chan struct{}, MaxConcurrentProviders)
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
			select {
			case semaphore <- struct{}{}:
				defer func() { <-semaphore }()
			case <-ctx.Done():
				pu.Error = ctx.Err().Error()
				report.Providers[i] = pu
				return
			}

			fetchCtx, cancel := context.WithTimeout(ctx, providerFetchTimeout)
			defer cancel()
			snap, err := RunStrategyChain(fetchCtx, p.Strategies())
			switch {
			case err == nil:
				pu.Snapshot = snap
			case fetchCtx.Err() == context.DeadlineExceeded:
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
