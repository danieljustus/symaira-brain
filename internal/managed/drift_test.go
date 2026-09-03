package managed

import (
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestCheckDrift_CurrentBehindAndUnknown(t *testing.T) {
	m := &Manifest{
		Cores: map[string]Core{
			"symvault":   {Repo: "danieljustus/symaira-vault", Version: "v0.15.3"},
			"symcockpit": {Repo: "danieljustus/symaira-cockpit", Version: "0.4.0"},
			"symdesk":    {Repo: "danieljustus/symaira-desktop", Version: "v0.11.1"},
		},
	}

	// Deliberately stale fixture: symvault's pin (v0.15.3) trails the
	// "latest" release (v0.21.1) returned here, reproducing the exact
	// drift found in the real manifest at the time this check was added.
	latest := func(repo string) (string, error) {
		switch repo {
		case "danieljustus/symaira-vault":
			return "v0.21.1", nil
		case "danieljustus/symaira-cockpit":
			return "0.4.0", nil
		case "danieljustus/symaira-desktop":
			return "", errors.New("network timeout")
		default:
			t.Fatalf("unexpected repo %q", repo)
			return "", nil
		}
	}

	results := CheckDrift(m, latest)
	if len(results) != 3 {
		t.Fatalf("expected 3 results, got %d", len(results))
	}

	byCore := make(map[string]DriftResult, len(results))
	for _, r := range results {
		byCore[r.Core] = r
	}

	if got := byCore["symvault"].Status; got != DriftBehind {
		t.Errorf("symvault: expected DriftBehind, got %s", got)
	}
	if got := byCore["symvault"].Latest; got != "v0.21.1" {
		t.Errorf("symvault: expected Latest=v0.21.1, got %q", got)
	}

	if got := byCore["symcockpit"].Status; got != DriftCurrent {
		t.Errorf("symcockpit: expected DriftCurrent, got %s", got)
	}

	if got := byCore["symdesk"].Status; got != DriftUnknown {
		t.Errorf("symdesk: expected DriftUnknown on network failure, got %s", got)
	}
	if byCore["symdesk"].Reason == "" {
		t.Error("symdesk: expected a non-empty Reason for DriftUnknown")
	}
}

func TestCheckDrift_NoReleasesIsUnknownNotDrift(t *testing.T) {
	m := &Manifest{Cores: map[string]Core{"x": {Repo: "o/r", Version: "v1.0.0"}}}
	latest := func(string) (string, error) { return "", nil }

	results := CheckDrift(m, latest)
	if results[0].Status != DriftUnknown {
		t.Errorf("expected DriftUnknown for a repo with no releases, got %s", results[0].Status)
	}
}

func TestCheckDrift_VPrefixNormalizedBeforeComparison(t *testing.T) {
	m := &Manifest{Cores: map[string]Core{"x": {Repo: "o/r", Version: "0.4.0"}}}
	latest := func(string) (string, error) { return "v0.4.0", nil }

	results := CheckDrift(m, latest)
	if results[0].Status != DriftCurrent {
		t.Errorf("expected DriftCurrent when only the v-prefix differs, got %s", results[0].Status)
	}
}

func TestCheckDrift_ResultsSortedByCoreName(t *testing.T) {
	m := &Manifest{Cores: map[string]Core{
		"z": {Repo: "o/z", Version: "v1.0.0"},
		"a": {Repo: "o/a", Version: "v1.0.0"},
	}}
	latest := func(string) (string, error) { return "v1.0.0", nil }

	results := CheckDrift(m, latest)
	if results[0].Core != "a" || results[1].Core != "z" {
		t.Errorf("expected results sorted [a z], got [%s %s]", results[0].Core, results[1].Core)
	}
}

func TestGitHubLatestTag(t *testing.T) {
	t.Run("returns the tag on 200", func(t *testing.T) {
		srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			if got := r.Header.Get("Authorization"); got != "Bearer test-token" {
				t.Errorf("expected Authorization header, got %q", got)
			}
			w.WriteHeader(http.StatusOK)
			w.Write([]byte(`{"tag_name":"v1.2.3"}`))
		}))
		defer srv.Close()

		fn := githubLatestTagWithBaseURL(srv.Client(), "test-token", srv.URL)
		tag, err := fn("o/r")
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if tag != "v1.2.3" {
			t.Errorf("expected tag v1.2.3, got %q", tag)
		}
	})

	t.Run("404 is unknown, not an error", func(t *testing.T) {
		srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			w.WriteHeader(http.StatusNotFound)
		}))
		defer srv.Close()

		fn := githubLatestTagWithBaseURL(srv.Client(), "", srv.URL)
		tag, err := fn("o/r")
		if err != nil {
			t.Fatalf("expected no error on 404, got %v", err)
		}
		if tag != "" {
			t.Errorf("expected empty tag on 404, got %q", tag)
		}
	})

	t.Run("non-200/404 is an error", func(t *testing.T) {
		srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			w.WriteHeader(http.StatusInternalServerError)
		}))
		defer srv.Close()

		fn := githubLatestTagWithBaseURL(srv.Client(), "", srv.URL)
		if _, err := fn("o/r"); err == nil {
			t.Error("expected an error on 500, got nil")
		}
	})
}
