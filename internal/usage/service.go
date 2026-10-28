package usage

import (
	"context"
	"fmt"
	"net/http"
	"os"
	"strings"
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

// AllProviders returns the historical ten-provider default graph in contract order.
// Additional backends require explicit selection through SelectedProviders.
func AllProviders(client *http.Client) []Provider {
	return allProviders(client, readClaudeKeychainCredential, shellProcessProbe{})
}

// allProviders is the production registry with the platform probes injected
// so report oracles can pin a machine-independent state without consulting a
// developer keychain or process table.
func allProviders(client *http.Client, readClaudeKeychain func() (string, *time.Time), antigravityProbe antigravityProcessProbe) []Provider {
	return providersForIDs(client, nil, readClaudeKeychain, antigravityProbe)
}

// ParseProviderSelection validates the entire selector without credential I/O.
// Duplicates are harmless; construction and reports retain catalog order.
func ParseProviderSelection(selector string) ([]string, error) {
	ids := strings.Split(selector, ",")
	for _, id := range ids {
		known := id == "zai" || id == "devin" || id == "grok" || id == "ollama"
		for _, provider := range contractProviders {
			if id == provider.ID {
				known = true
				break
			}
		}
		if !known {
			return nil, fmt.Errorf("invalid provider ID %q", id)
		}
	}
	return ids, nil
}

// SelectedProviders validates before constructing only the requested providers.
func SelectedProviders(client *http.Client, selector string) ([]Provider, error) {
	ids, err := ParseProviderSelection(selector)
	if err != nil {
		return nil, err
	}
	return providersForIDs(client, ids, readClaudeKeychainCredential, shellProcessProbe{}), nil
}

func providersForIDs(client *http.Client, ids []string, readClaudeKeychain func() (string, *time.Time), antigravityProbe antigravityProcessProbe) []Provider {
	if client == nil {
		client = newProviderHTTPClient()
	}
	factories := []func() Provider{
		func() Provider { return newClaudeProvider(client, readClaudeKeychain) },
		func() Provider { return NewCodexProvider(client) },
		func() Provider { return NewCopilotProvider(client) },
		func() Provider { return NewCursorProvider(client) },
		func() Provider { return NewKimiProvider(client) },
		func() Provider { return NewMoonshotProvider(client) },
		func() Provider { return NewNousPortalProvider(client) },
		func() Provider { return newCurrentOpenCodeProvider(client) },
		func() Provider { return NewOpenRouterProvider(client) },
		func() Provider { return newAntigravityProvider(antigravityProbe, client) },
	}
	providers := make([]Provider, 0, len(factories))
	for i, factory := range factories {
		if ids != nil {
			selected := false
			for _, id := range ids {
				if id == contractProviders[i].ID {
					selected = true
					break
				}
			}
			if !selected {
				continue
			}
		}
		var p Provider
		if ids != nil && contractProviders[i].ID == "claude" && os.Getenv("SYMBRAIN_USAGE_CLAUDE_HISTORY") == "1" && os.Getenv("SYMBRAIN_USAGE_CLAUDE_HISTORY_ONLY") == "1" {
			p = &ClaudeProvider{client: client}
		} else if ids != nil && contractProviders[i].ID == "codex" && os.Getenv("SYMBRAIN_USAGE_CODEX_HISTORY") == "1" && os.Getenv("SYMBRAIN_USAGE_CODEX_HISTORY_ONLY") == "1" {
			p = &CodexProvider{client: client, localHistoryOnly: true}
		} else {
			p = factory()
		}
		if ids != nil && (p.ID() == "opencode" || p.ID() == "claude" || p.ID() == "codex") {
			p = selectedHistoryProvider{p}
		}
		providers = append(providers, p)
	}
	for _, factory := range []struct {
		id   string
		make func(*http.Client) Provider
	}{
		{"zai", func(c *http.Client) Provider { return NewZAIProvider(c) }},
		{"devin", func(c *http.Client) Provider { return NewDevinProvider(c) }},
		{"grok", func(c *http.Client) Provider { return NewGrokProvider(c) }},
		{"ollama", func(c *http.Client) Provider { return NewOllamaProvider(c) }},
	} {
		for _, selected := range ids {
			if selected == factory.id {
				providers = append(providers, factory.make(client))
				break
			}
		}
	}
	return providers
}

type selectedHistoryProvider struct{ Provider }

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
		if p.ID() == "grok" && os.Getenv("SYMBRAIN_USAGE_GROK_HISTORY") == "1" {
			pu.History = ScanGrokHistory(ctx, grokHistoryHome(), time.Now())
		}
		if _, selected := p.(selectedHistoryProvider); selected && p.ID() == "opencode" && os.Getenv("SYMBRAIN_USAGE_OPENCODE_HISTORY") == "1" {
			pu.History = ScanOpenCodeHistory(ctx, openCodeHistoryHome(), time.Now())
		}
		if _, selected := p.(selectedHistoryProvider); selected && p.ID() == "claude" && os.Getenv("SYMBRAIN_USAGE_CLAUDE_HISTORY") == "1" {
			pu.History = ScanClaudeHistory(ctx, claudeHistoryHome(), time.Now())
		}
		if _, selected := p.(selectedHistoryProvider); selected && p.ID() == "codex" && os.Getenv("SYMBRAIN_USAGE_CODEX_HISTORY") == "1" {
			pu.History = ScanCodexHistory(ctx, codexHomeDir(), time.Now())
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
