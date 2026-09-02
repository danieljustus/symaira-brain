package usage

import (
	"context"
	"strings"
	"testing"
	"time"
)

type fakeProvider struct {
	id         string
	name       string
	configured bool
	strategies []Strategy
	authStatus AuthStatus
}

func (p *fakeProvider) ID() string             { return p.id }
func (p *fakeProvider) DisplayName() string    { return p.name }
func (p *fakeProvider) IsConfigured() bool     { return p.configured }
func (p *fakeProvider) Strategies() []Strategy { return p.strategies }
func (p *fakeProvider) AuthStatus() AuthStatus { return p.authStatus }

type fakeStrategy struct {
	source string
	snap   *UsageSnapshot
	err    error
}

func (s *fakeStrategy) Source() string { return s.source }
func (s *fakeStrategy) Fetch(context.Context) (*UsageSnapshot, error) {
	if s.err != nil {
		return nil, s.err
	}
	return s.snap, nil
}

func TestBuildReportSkipsFetchForUnconfiguredProviders(t *testing.T) {
	providers := []Provider{
		&fakeProvider{id: "x", name: "X", configured: false, authStatus: AuthStatus{Status: "missing"}},
	}
	report := BuildReport(context.Background(), providers)

	if len(report.Providers) != 1 {
		t.Fatalf("Providers = %d, want 1", len(report.Providers))
	}
	pu := report.Providers[0]
	if pu.Configured {
		t.Error("expected Configured=false")
	}
	if pu.Snapshot != nil {
		t.Error("expected no fetch attempt for an unconfigured provider")
	}
	if pu.Error != "" {
		t.Errorf("Error = %q, want empty (no fetch attempted)", pu.Error)
	}
}

func TestBuildReportRecordsSuccessfulSnapshot(t *testing.T) {
	providers := []Provider{
		&fakeProvider{
			id: "x", name: "X", configured: true,
			strategies: []Strategy{&fakeStrategy{source: "api", snap: &UsageSnapshot{ProviderID: "x"}}},
			authStatus: AuthStatus{Status: "available"},
		},
	}
	report := BuildReport(context.Background(), providers)

	pu := report.Providers[0]
	if pu.Snapshot == nil {
		t.Fatal("expected a snapshot")
	}
	if pu.Snapshot.Source != "api" {
		t.Errorf("Source = %q, want api", pu.Snapshot.Source)
	}
	if pu.Error != "" {
		t.Errorf("Error = %q, want empty", pu.Error)
	}
}

func TestBuildReportRecordsChainFailure(t *testing.T) {
	providers := []Provider{
		&fakeProvider{
			id: "x", name: "X", configured: true,
			strategies: []Strategy{&fakeStrategy{source: "api", err: errBoom}},
			authStatus: AuthStatus{Status: "available"},
		},
	}
	report := BuildReport(context.Background(), providers)

	pu := report.Providers[0]
	if pu.Snapshot != nil {
		t.Error("expected no snapshot on failure")
	}
	if pu.Error == "" {
		t.Error("expected an error recorded on the ProviderUsage")
	}
}

// slowFakeStrategy simulates a stalled provider: it blocks until either its
// delay elapses (success) or the fetch context is cancelled/times out
// first — same shape as a real strategy waiting on a slow HTTP round trip.
type slowFakeStrategy struct {
	source string
	delay  time.Duration
	snap   *UsageSnapshot
}

func (s *slowFakeStrategy) Source() string { return s.source }
func (s *slowFakeStrategy) Fetch(ctx context.Context) (*UsageSnapshot, error) {
	select {
	case <-time.After(s.delay):
		return s.snap, nil
	case <-ctx.Done():
		return nil, ctx.Err()
	}
}

// TestBuildReportBoundsAStalledProviderWithoutBlockingOthers is the issue
// #429 regression test: before the fan-out fix, BuildReport awaited each
// provider's strategy chain in sequence, so a single stalled provider (here
// simulated with a strategy that sleeps well past the per-provider budget)
// consumed the entire run and delayed every provider queued behind it. It
// asserts the fast provider still completes, on a total runtime bounded by
// the (shrunk, for test speed) per-provider timeout rather than by the
// stalled provider's delay, and that the stalled provider is reported with
// a dedicated timeout error instead of an empty/starved result.
func TestBuildReportBoundsAStalledProviderWithoutBlockingOthers(t *testing.T) {
	origTimeout := providerFetchTimeout
	providerFetchTimeout = 50 * time.Millisecond
	defer func() { providerFetchTimeout = origTimeout }()

	const stallDelay = 2 * time.Second // far longer than the shrunk per-provider timeout
	providers := []Provider{
		&fakeProvider{
			id: "slow", name: "Slow", configured: true,
			strategies: []Strategy{&slowFakeStrategy{source: "api", delay: stallDelay}},
			authStatus: AuthStatus{Status: "available"},
		},
		&fakeProvider{
			id: "fast", name: "Fast", configured: true,
			strategies: []Strategy{&fakeStrategy{source: "api", snap: &UsageSnapshot{ProviderID: "fast"}}},
			authStatus: AuthStatus{Status: "available"},
		},
	}

	start := time.Now()
	report := BuildReport(context.Background(), providers)
	elapsed := time.Since(start)

	// Serialized execution would take >= stallDelay before even reaching the
	// fast provider; a bounded fan-out completes in roughly one per-provider
	// timeout regardless of how many providers came before the stalled one.
	if elapsed >= stallDelay {
		t.Fatalf("BuildReport took %s, want well under the stalled provider's %s delay — the per-provider timeout should have bounded it", elapsed, stallDelay)
	}

	if len(report.Providers) != 2 {
		t.Fatalf("Providers = %d, want 2", len(report.Providers))
	}

	slow := report.Providers[0]
	if slow.ID != "slow" {
		t.Fatalf("Providers[0].ID = %q, want %q (report order must match input order)", slow.ID, "slow")
	}
	if slow.Snapshot != nil {
		t.Error("expected no snapshot for the stalled provider")
	}
	if slow.Error == "" || !strings.Contains(slow.Error, "timed out") {
		t.Errorf("Error = %q, want a timeout error mentioning \"timed out\"", slow.Error)
	}

	fast := report.Providers[1]
	if fast.ID != "fast" {
		t.Fatalf("Providers[1].ID = %q, want %q (report order must match input order)", fast.ID, "fast")
	}
	if fast.Snapshot == nil {
		t.Fatal("expected the fast provider to complete despite the stalled provider ahead of it")
	}
	if fast.Error != "" {
		t.Errorf("Error = %q, want empty", fast.Error)
	}
}

func TestAllProvidersReturnsTenProviders(t *testing.T) {
	providers := AllProviders(nil)
	if len(providers) != 10 {
		t.Fatalf("AllProviders() = %d, want 10", len(providers))
	}
	seen := map[string]bool{}
	for _, p := range providers {
		if seen[p.ID()] {
			t.Errorf("duplicate provider id %q", p.ID())
		}
		seen[p.ID()] = true
	}
}
