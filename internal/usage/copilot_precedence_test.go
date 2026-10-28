package usage

import (
	"os"
	"path/filepath"
	"testing"
)

func TestCopilotModernSelectionPrecedence(t *testing.T) {
	for _, tc := range []struct{ name, modern, env, want string }{
		{"settings do not hide legacy", `{}`, "", "fixture-legacy"},
		{"empty user list does not hide legacy", `{"loggedInUsers":[]}`, "", "fixture-legacy"},
		{"selected login does not use other legacy account", `{"loggedInUsers":[{"host":"https://github.com","login":"selected"}]}`, "", ""},
		{"environment preempts selection", `{"loggedInUsers":[{"host":"https://github.com","login":"selected"}]}`, "fixture-env", "fixture-env"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			home := t.TempDir()
			t.Setenv("HOME", home)
			t.Setenv("COPILOT_ACCESS_TOKEN", tc.env)
			for path, data := range map[string]string{
				filepath.Join(home, ".copilot/config.json"):             tc.modern,
				filepath.Join(home, ".config/github-copilot/apps.json"): `{"github.com:old":{"oauth_token":"fixture-legacy"}}`,
			} {
				if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
					t.Fatal(err)
				}
				if err := os.WriteFile(path, []byte(data), 0600); err != nil {
					t.Fatal(err)
				}
			}
			calls := 0
			p := newCopilotProvider(nil, func(string) string { calls++; return "" })
			if p.accessToken != tc.want {
				t.Error("precedence mismatch")
			}
			if tc.env != "" && calls != 0 {
				t.Error("env must skip keychain")
			}
		})
	}
}
