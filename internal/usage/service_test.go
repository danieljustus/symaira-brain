package usage

import (
	"context"
	"testing"
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

func TestAllProvidersReturnsThreeProviders(t *testing.T) {
	providers := AllProviders(nil)
	if len(providers) != 3 {
		t.Fatalf("AllProviders() = %d, want 3 (issue #290 progress: 8 more remain)", len(providers))
	}
}
