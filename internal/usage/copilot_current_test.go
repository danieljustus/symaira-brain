package usage

import (
	"os"
	"path/filepath"
	"testing"
)

func TestCopilotCurrentCLI(t *testing.T) {
	for _, tc := range []struct{ name, data, wantAccount, wantToken string }{
		{"selected", `{// comment
 "loggedInUsers":[{"host":"https://github.com","login":"first"},{"host":"https://github.com","login":"second"}],
 "lastLoggedInUser":{"host":"https://github.com","login":"second"},
 }`, "https://github.com:second", "fixture-selected"},
		{"single", `{"loggedInUsers":[{"host":"https://github.com","login":"first"}]}`, "https://github.com:first", "fixture-selected"},
		{"ambiguous", `{"loggedInUsers":[{"host":"https://github.com","login":"first"},{"host":"https://github.com","login":"second"}]}`, "", ""},
		{"unlisted selection", `{"loggedInUsers":[{"host":"https://github.com","login":"first"}],"lastLoggedInUser":{"host":"https://github.com","login":"absent"}}`, "", ""},
	} {
		t.Run(tc.name, func(t *testing.T) {
			home := t.TempDir()
			t.Setenv("HOME", home)
			t.Setenv("COPILOT_ACCESS_TOKEN", "")
			dir := filepath.Join(home, ".copilot")
			if err := os.MkdirAll(dir, 0700); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(filepath.Join(dir, "config.json"), []byte(tc.data), 0600); err != nil {
				t.Fatal(err)
			}
			calls := 0
			p := newCopilotProvider(nil, func(account string) string {
				calls++
				if account != tc.wantAccount {
					t.Errorf("wrong account")
				}
				return "fixture-selected"
			})
			if p.accessToken != tc.wantToken {
				t.Error("selected token mismatch")
			}
			if tc.wantAccount == "" && calls != 0 {
				t.Error("ambiguous config queried keychain")
			}
		})
	}
}
