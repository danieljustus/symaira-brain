package usage

import (
	"reflect"
	"testing"
	"time"
)

func TestParseProviderSelection(t *testing.T) {
	for _, selector := range []string{"", ",", "codex,", ",codex", "codex,unknown", "Codex", " codex"} {
		if _, err := ParseProviderSelection(selector); err == nil {
			t.Errorf("accepted %q", selector)
		}
	}
	for _, selector := range []string{"codex", "openrouter,codex,codex"} {
		ids, err := ParseProviderSelection(selector)
		if err != nil {
			t.Fatal(err)
		}
		t.Setenv("CODEX_ACCESS_TOKEN", "")
		t.Setenv("OPENROUTER_API_KEY", "")
		t.Setenv("CODEX_HOME", t.TempDir())
		providers := providersForIDs(nil, ids, func() (string, *time.Time) {
			t.Fatal("excluded Claude Keychain accessed")
			return "", nil
		}, nil) // An excluded Antigravity probe must never be constructed.
		got := make([]string, len(providers))
		for i, provider := range providers {
			got[i] = provider.ID()
		}
		want := []string{"codex"}
		if selector != "codex" {
			want = append(want, "openrouter")
		}
		if !reflect.DeepEqual(got, want) {
			t.Fatalf("%q: got %v, want %v", selector, got, want)
		}
	}
}
