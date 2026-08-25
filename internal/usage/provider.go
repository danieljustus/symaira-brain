package usage

import "context"

// Provider is a usage provider: the contract every AI-provider client
// implements against. Mirrors symaira-cockpit's AIUsageProvider protocol
// (tune/Sources/SymTuneCore/AIUsage.swift:121-140), minus the UI-facing
// credentialDescriptor (a live closure, not something a CLI/MCP report
// needs) — AuthStatus is the static snapshot equivalent.
type Provider interface {
	ID() string
	DisplayName() string
	// IsConfigured reports whether the provider has a usable credential.
	// Unconfigured providers must be reported as "not set up", never as an
	// error.
	IsConfigured() bool
	// Strategies returns the ordered fallback strategies; empty when the
	// provider is not configured.
	Strategies() []Strategy
	// AuthStatus describes how (or whether) the provider's credential
	// resolved, for a caller that wants to show why without understanding
	// any provider's auth flow.
	AuthStatus() AuthStatus
}

// Strategy is one ordered fallback strategy of a provider. Strategies run
// in order and the first success wins; failures are collected, not
// swallowed. Mirrors AIUsageStrategy (AIUsage.swift:114-118).
type Strategy interface {
	// Source is the human-readable source tag (oauth, cli, web, api, local).
	Source() string
	Fetch(ctx context.Context) (*UsageSnapshot, error)
}

// RunStrategyChain runs strategies in order and returns the first success,
// tagged with the winning strategy's source. When every strategy fails, it
// returns a ChainFailedError carrying every partial error. Mirrors
// AIUsageStrategyChain.run (AIUsage.swift:222-238).
func RunStrategyChain(ctx context.Context, strategies []Strategy) (*UsageSnapshot, error) {
	var failures []string
	for _, s := range strategies {
		snap, err := s.Fetch(ctx)
		if err == nil {
			snap.Source = s.Source()
			return snap, nil
		}
		failures = append(failures, err.Error())
	}
	return nil, &ChainFailedError{Failures: failures}
}
