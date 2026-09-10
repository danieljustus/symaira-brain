package usage

import (
	"context"
	"errors"
	"net/http"
	"os/exec"
	"sync/atomic"
	"testing"
	"time"
)

func TestBoundedCommandOutputAlwaysCapsParentDeadline(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	started := time.Now()
	if output, ok := boundedCommandOutput(ctx, func(ctx context.Context) *exec.Cmd {
		return exec.CommandContext(ctx, "/bin/sh", "-c", "sleep 5")
	}); ok || output != "" {
		t.Fatalf("boundedCommandOutput = (%q, %v), want timeout failure", output, ok)
	}
	if elapsed := time.Since(started); elapsed >= 3*time.Second {
		t.Fatalf("boundedCommandOutput took %s, want the 2-second cap", elapsed)
	}
}

func TestTrustedHTTPSURLRejectsPrivateLocalAndReservedOrigins(t *testing.T) {
	for _, raw := range []string{
		"http://api.example.com/usage",
		"https://127.0.0.1/usage",
		"https://10.0.0.1/usage",
		"https://172.16.0.1/usage",
		"https://192.168.1.1/usage",
		"https://169.254.1.1/usage",
		"https://192.0.2.1/usage",
		"https://203.0.113.1/usage",
		"https://[::1]/usage",
		"https://[fd00::1]/usage",
		"https://user:pass@example.com/usage",
		"https://api.example.com.local/usage",
		"https://api.example.com:bad/usage",
	} {
		if trustedHTTPSURL(raw, false) {
			t.Errorf("trustedHTTPSURL(%q) accepted unsafe origin", raw)
		}
	}
	if !trustedHTTPSURL("https://127.0.0.1/usage", true) {
		t.Error("explicit loopback opt-in was rejected")
	}
	if !trustedHTTPSURL("https://api.example.com/usage", false) {
		t.Error("public HTTPS origin was rejected")
	}
	if !trustedHTTPSURL("https://198.51.99.1/usage", false) {
		t.Error("non-reserved 198.51/16 address was rejected")
	}
}

func TestFetchJSONBoundsResponseBody(t *testing.T) {
	client, _ := fakeClient(http.StatusOK, make([]byte, maxUsageResponseBytes+1), nil)
	req, err := http.NewRequest(http.MethodGet, "https://api.example.com/usage", nil)
	if err != nil {
		t.Fatal(err)
	}
	_, err = FetchJSON[map[string]any](context.Background(), req, "test", client)
	var httpErr *HTTPError
	if !errors.As(err, &httpErr) || httpErr.Kind != "unparseable" {
		t.Fatalf("FetchJSON oversized response error = %v, want unparseable HTTPError", err)
	}
}

func TestRunStrategyChainRejectsSuccessfulNoMeterPayload(t *testing.T) {
	_, err := RunStrategyChain(context.Background(), []Strategy{
		&fakeStrategy{source: "api", snap: &UsageSnapshot{ProviderID: "malformed"}},
	})
	if err == nil || !stringsContains(err.Error(), "no usable usage fields") {
		t.Fatalf("RunStrategyChain error = %v, want no-meter payload error", err)
	}
}

func TestBuildReportLimitsGlobalProviderConcurrency(t *testing.T) {
	var active, maxActive atomic.Int32
	providers := make([]Provider, 8)
	for i := range providers {
		id := "provider-" + string(rune('a'+i))
		providers[i] = &fakeProvider{
			id: id, name: id, configured: true,
			strategies: []Strategy{&countingStrategy{active: &active, maxActive: &maxActive}},
			authStatus: AuthStatus{Status: "available"},
		}
	}
	report := BuildReport(context.Background(), providers)
	if len(report.Providers) != len(providers) {
		t.Fatalf("report providers = %d, want %d", len(report.Providers), len(providers))
	}
	if got := maxActive.Load(); got > MaxConcurrentProviders {
		t.Fatalf("maximum concurrent providers = %d, want <= %d", got, MaxConcurrentProviders)
	}
}

type countingStrategy struct {
	active, maxActive *atomic.Int32
}

func (s *countingStrategy) Source() string { return "api" }
func (s *countingStrategy) Fetch(ctx context.Context) (*UsageSnapshot, error) {
	current := s.active.Add(1)
	for {
		old := s.maxActive.Load()
		if current <= old || s.maxActive.CompareAndSwap(old, current) {
			break
		}
	}
	defer s.active.Add(-1)
	select {
	case <-time.After(10 * time.Millisecond):
		return &UsageSnapshot{ProviderID: "counted", Meters: []UsageMeter{{Label: "requests", Unit: "requests"}}}, nil
	case <-ctx.Done():
		return nil, ctx.Err()
	}
}

func stringsContains(value, needle string) bool {
	for i := 0; i+len(needle) <= len(value); i++ {
		if value[i:i+len(needle)] == needle {
			return true
		}
	}
	return false
}
