package usage

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"runtime"
	"testing"
)

// Shared with Rust: same source bytes, selection and literal token contract.
func TestCopilotCurrentSharedCorpus(t *testing.T) {
	_, source, _, _ := runtime.Caller(0)
	data, err := os.ReadFile(filepath.Join(filepath.Dir(source), "../../rust/symbrain-usage/tests/fixtures/current_copilot_cli.json"))
	if err != nil {
		t.Fatal(err)
	}
	var cases []struct{ ID, Data, Account, Token string }
	if err := json.Unmarshal(data, &cases); err != nil {
		t.Fatal(err)
	}
	if len(cases) != 11 {
		t.Fatalf("cases = %d", len(cases))
	}
	for _, tc := range cases {
		t.Run(tc.ID, func(t *testing.T) {
			account := copilotCLIAccount([]byte(tc.Data))
			if account != tc.Account {
				t.Error("account mismatch")
			}
			token := ""
			if account != "" {
				token = copilotCLIFileToken([]byte(tc.Data), account)
			}
			if token != tc.Token {
				t.Error("literal token mismatch")
			}
			home := t.TempDir()
			t.Setenv("HOME", home)
			t.Setenv("COPILOT_ACCESS_TOKEN", "")
			dir := filepath.Join(home, ".copilot")
			if err := os.MkdirAll(dir, 0700); err != nil {
				t.Fatal(err)
			}
			path := filepath.Join(dir, "config.json")
			if err := os.WriteFile(path, []byte(tc.Data), 0600); err != nil {
				t.Fatal(err)
			}
			calls := 0
			client, transport := fakeClient(200, []byte(`{"quota_snapshots":{"premium_interactions":{"has_quota":true,"entitlement":100,"remaining":75}}}`), nil)
			p := newCopilotProvider(client, func(string) string { calls++; return "" })
			if tc.Token != "" {
				report := BuildReport(context.Background(), []Provider{p})
				usage := report.Providers[0]
				if !usage.Configured || usage.Error != "" || usage.Snapshot == nil || len(usage.Snapshot.Meters) != 1 || *usage.Snapshot.Meters[0].Used != "25" {
					t.Fatal("current CLI credential did not return fixture usage")
				}
				if transport.lastRequest.Header.Get("Authorization") != "Bearer "+tc.Token {
					t.Fatal("wrong fixture authorization")
				}
			}
			if p.accessToken != tc.Token {
				t.Error("constructor token mismatch")
			}
			if tc.Token == "" && tc.ID != "signed-out" && p.AuthStatus().Detail != "credential resolution failed: "+copilotCLILoginError {
				t.Errorf("status = %+v", p.AuthStatus())
			}
			if tc.Token != "" && calls != 0 {
				t.Error("plaintext source must preempt Keychain")
			}
			after, err := os.ReadFile(path)
			if err != nil || string(after) != tc.Data {
				t.Error("store mutated")
			}
		})
	}
}
