package usage

import "context"

// Provider is a usage provider: the contract every AI-provider client
// implements against.
type Provider interface {
	ID() string
	DisplayName() string
	IsConfigured() bool
	Strategies() []Strategy
	AuthStatus() AuthStatus
}

// Strategy is one ordered fallback strategy of a provider. Strategies are
// intentionally run serially: a provider has at most one in-flight request.
type Strategy interface {
	Source() string
	Fetch(ctx context.Context) (*UsageSnapshot, error)
}

// RunStrategyChain runs strategies in order and returns the first successful,
// usable snapshot. A 2xx response with no meters and no balance is malformed,
// not a successful zero-usage result.
func RunStrategyChain(ctx context.Context, strategies []Strategy) (*UsageSnapshot, error) {
	var failures []string
	for _, strategy := range strategies {
		if err := ctx.Err(); err != nil {
			failures = append(failures, err.Error())
			break
		}
		snap, err := strategy.Fetch(ctx)
		if err == nil {
			if snap == nil {
				err = &PayloadError{Detail: "empty snapshot"}
			} else {
				snap.Source = strategy.Source()
				if len(snap.Meters) == 0 && snap.Balance == nil {
					err = &PayloadError{ProviderID: snap.ProviderID, Detail: "response contained no usable usage fields"}
				}
			}
		}
		if err == nil {
			return snap, nil
		}
		failures = append(failures, err.Error())
	}
	return nil, &ChainFailedError{Failures: failures}
}
