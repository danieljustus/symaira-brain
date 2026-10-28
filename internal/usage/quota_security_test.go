package usage

import (
	"context"
	"errors"
	"net/http"
	"strings"
	"testing"
)

func TestQuotaHTTPFailuresAreRedacted(t *testing.T) {
	t.Setenv("ZAI_API_KEY", "hermetic-secret")
	t.Setenv("DEVIN_API_KEY", "hermetic-secret")
	for _, id := range []string{"zai", "devin"} {
		for _, status := range []int{302, 401, 403, 429, 500} {
			t.Run(id+http.StatusText(status), func(t *testing.T) {
				c, tr := fakeClient(status, []byte(`{"echo":"hermetic-secret"}`), map[string]string{"Location": "https://untrusted.example/steal", "Retry-After": "30"})
				var p Provider = NewZAIProvider(c)
				if id == "devin" {
					p = NewDevinProvider(c)
				}
				_, err := p.Strategies()[0].Fetch(context.Background())
				if err == nil || strings.Contains(err.Error(), "hermetic-secret") {
					t.Fatal("missing or leaking error")
				}
				if status == 302 {
					var e *HTTPError
					if !errors.As(err, &e) || e.Status != 302 || tr.lastRequest.URL.Host == "untrusted.example" {
						t.Fatal("redirect followed")
					}
				}
				if status == 429 {
					var e *RateLimitedError
					if !errors.As(err, &e) {
						t.Fatal(err)
					}
				}
			})
		}
	}
	c, _ := fakeErrorClient(errors.New("hermetic-secret"))
	p := NewDevinProvider(c)
	_, err := p.Strategies()[0].Fetch(context.Background())
	if err == nil || strings.Contains(err.Error(), "hermetic-secret") {
		t.Fatal("transport leak")
	}
}
