package usage

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

func TestNousCurrentHermesStore(t *testing.T) {
	for _, tc := range []struct{ name, data, token string }{
		{"quarantined", `{"providers":{"nous":{"access_token":"fixture-stale","last_auth_error":{"message":"private diagnostic"}}},"credential_pool":{"nous":[{"access_token":"fixture-other"}]}}`, ""},
		{"portal not inference", `{"providers":{"nous":{"access_token":"fixture-portal","agent_key":"fixture-inference","invoke_jwt":"fixture-invoke"}}}`, "fixture-portal"},
		{"current", `{"providers":{"nous":{"access_token":"fixture-portal"}}}`, "fixture-portal"},
		{"pool", `{"credential_pool":{"nous":[{"access_token":"fixture-pool"}]}}`, "fixture-pool"},
		{"signed out", `{"providers":{"nous":{"last_auth_error":{"message":"private diagnostic"}}}}`, ""},
		{"expired", `{"providers":{"nous":{"access_token":"fixture-old","expires_at":"2000-01-01T00:00:00Z"}}}`, ""},
	} {
		t.Run(tc.name, func(t *testing.T) {
			dir := t.TempDir()
			path := filepath.Join(dir, "auth.json")
			if err := os.WriteFile(path, []byte(tc.data), 0600); err != nil {
				t.Fatal(err)
			}
			if got := readNousAccessToken(path); got != tc.token {
				t.Errorf("token presence/match incorrect")
			}
			t.Setenv("HERMES_HOME", dir)
			t.Setenv("NOUS_PORTAL_ACCESS_TOKEN", "")
			client, transport := fakeClient(200, []byte(`{"paid_service_access":{"total_usable_credits":42}}`), nil)
			p := NewNousPortalProvider(client)
			if tc.token != "" {
				usage := BuildReport(context.Background(), []Provider{p}).Providers[0]
				if !usage.Configured || usage.Error != "" || usage.Snapshot == nil || usage.Snapshot.Balance == nil || *usage.Snapshot.Balance != "42" {
					t.Fatal("current Hermes login did not return fixture credits")
				}
				if transport.lastRequest.Header.Get("Authorization") != "Bearer "+tc.token {
					t.Fatal("wrong fixture authorization")
				}
			}
			if tc.token == "" && p.AuthStatus().Detail != "Hermes Nous login missing/expired — sign in again with the Hermes CLI" {
				t.Errorf("status = %+v", p.AuthStatus())
			}
		})
	}
}
