package usage

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"testing"
)

func TestKimiCLIExpiry(t *testing.T) {
	const expired = "Kimi Code CLI login expired — run the Kimi CLI once to refresh it"
	const rejected = "Kimi rejected the login (HTTP 401). Check the API access or sign in with the Kimi Code CLI again."
	for _, tc := range []struct {
		name, expiry, want string
		requests           bool
	}{
		{"expired_timestamp", `"2000-01-01T00:00:00Z"`, expired, false},
		{"expired_seconds", `946684800.5`, expired, false},
		{"unexpired_401", `"2999-01-01T00:00:00Z"`, rejected, true},
		{"unexpired_seconds_401", `32472144000`, rejected, true},
		{"missing_401", `null`, rejected, true},
		{"invalid_401", `"unknown"`, rejected, true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			home := t.TempDir()
			t.Setenv("HOME", home)
			t.Setenv("USERPROFILE", home)
			t.Setenv("KIMI_CODE_HOME", "")
			t.Setenv("KIMI_CODE_API_KEY", "")
			t.Setenv("KIMI_AUTH_TOKEN", "")
			path := filepath.Join(home, ".kimi", "credentials", "kimi-code.json")
			if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
				t.Fatal(err)
			}
			before := []byte(fmt.Sprintf(`{"access_token":"synthetic-cli","refresh_token":"do-not-touch","expires_at":%s}`, tc.expiry))
			if err := os.WriteFile(path, before, 0600); err != nil {
				t.Fatal(err)
			}
			client, transport := fakeClient(401, []byte("unauthorized"), nil)
			provider := NewKimiProvider(client)
			strategies := provider.Strategies()
			if len(strategies) != 1 || strategies[0].Source() != "cli" {
				t.Fatalf("unexpected strategies: %v", strategies)
			}
			_, err := RunStrategyChain(context.Background(), strategies)
			want := "all AI usage fallbacks failed: " + tc.want
			if err == nil || err.Error() != want {
				t.Errorf("Fetch error = %v, want %s", err, want)
			}
			if (transport.lastRequest != nil) != tc.requests {
				t.Errorf("request made = %v, want %v", transport.lastRequest != nil, tc.requests)
			}
			after, err := os.ReadFile(path)
			if err != nil {
				t.Fatal(err)
			}
			if string(after) != string(before) {
				t.Fatal("CLI credential file changed")
			}
		})
	}
}
