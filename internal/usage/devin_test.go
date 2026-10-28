package usage

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestDevinQuota(t *testing.T) {
	cases := []struct {
		name, fields string
		used         string
		bad          bool
	}{
		{"absent", ``, "", true},
		{"zero", `"weeklyQuotaRemainingPercent":0`, "100", false},
		{"omittedZero", `"weeklyQuotaResetAtUnix":"1770648402"`, "100", false},
		{"fallback", `"planInfo":{"hideDailyQuota":true},"dailyQuotaRemainingPercent":70`, "30", false},
		{"notFallback", `"planInfo":{"hideDailyQuota":true},"dailyQuotaRemainingPercent":70,"weeklyQuotaResetAtUnix":1770648402`, "100", false},
		{"malformed", `"weeklyQuotaRemainingPercent":"bad","weeklyQuotaResetAtUnix":1770648402`, "", true},
		{"null", `"weeklyQuotaRemainingPercent":null`, "", true},
		{"boolean", `"weeklyQuotaRemainingPercent":false`, "", true},
		{"badReset", `"weeklyQuotaRemainingPercent":10,"weeklyQuotaResetAtUnix":"oops"`, "", true},
		{"balanceZero", `"overageBalanceMicros":"0"`, "0", false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			var root map[string]json.RawMessage
			err := json.Unmarshal([]byte(`{"userStatus":{"planStatus":{`+tc.fields+`}}}`), &root)
			if err != nil {
				t.Fatal(err)
			}
			s, err := parseDevinQuota(root)
			if (err != nil) != tc.bad {
				t.Fatalf("%v %v", s, err)
			}
			if err == nil && *s.Meters[0].Used != tc.used {
				t.Fatal(s.Meters)
			}
		})
	}
}
func TestDevinCredentialsAndRequest(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("USERPROFILE", home)
	t.Setenv("DEVIN_API_KEY", "")
	if NewDevinProvider(nil).IsConfigured() {
		t.Fatal("missing configured")
	}
	path := filepath.Join(home, ".local", "share", "devin", "credentials.toml")
	if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte("windsurf_api_key = 'file-key'\napi_server_url = 'https://untrusted.example'\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if p := NewDevinProvider(nil); !p.IsConfigured() || p.AuthStatus().Source != "file" {
		t.Fatal(p.AuthStatus())
	}
	old := vaultResolver
	t.Cleanup(func() { vaultResolver = old })
	vaultResolver = func(string, string) (string, error) { return "env-key", nil }
	t.Setenv("DEVIN_API_KEY", "symvault://test/devin")
	c, tr := fakeClient(200, []byte(`{"userStatus":{"planStatus":{"weeklyQuotaRemainingPercent":0}}}`), nil)
	p := NewDevinProvider(c)
	if p.AuthStatus().Source != "vault" {
		t.Fatal(p.AuthStatus())
	}
	if _, err := p.Strategies()[0].Fetch(context.Background()); err != nil {
		t.Fatal(err)
	}
	r := tr.lastRequest
	if r.Method != "POST" || r.URL.String() != "https://server.codeium.com/exa.seat_management_pb.SeatManagementService/GetUserStatus" || r.Header.Get("Connect-Protocol-Version") != "1" || r.Header.Get("Content-Type") != "application/json" || r.Header.Get("Authorization") != "" {
		t.Fatal("request contract")
	}
	b, err := io.ReadAll(r.Body)
	if err != nil {
		t.Fatal(err)
	}
	var body map[string]map[string]string
	if err = json.Unmarshal(b, &body); err != nil {
		t.Fatal(err)
	}
	want := map[string]string{"apiKey": "env-key", "ideName": "devin", "ideVersion": "1.108.2", "extensionName": "devin", "extensionVersion": "1.108.2", "locale": "en"}
	if len(body) != 1 || len(body["metadata"]) != len(want) {
		t.Fatal("metadata shape")
	}
	for k, v := range want {
		if body["metadata"][k] != v {
			t.Fatal(k)
		}
	}
	vaultResolver = func(string, string) (string, error) { return "", errors.New("env-key") }
	p = NewDevinProvider(nil)
	if p.IsConfigured() || strings.Contains(p.AuthStatus().Detail, "env-key") {
		t.Fatal("error fallback or leak")
	}
	for _, content := range []string{"windsurf_api_key = [", "windsurf_api_key = 12", "windsurf_api_key = ''", "windsurf_api_key = 'a'\nwindsurf_api_key = 'b'"} {
		if err = os.WriteFile(path, []byte(content), 0600); err != nil {
			t.Fatal(err)
		}
		if devinFileKey(path) != "" {
			t.Fatal("bad file accepted")
		}
	}
	if err = os.WriteFile(path, []byte("windsurf_api_key = 'file-key'"), 0600); err != nil {
		t.Fatal(err)
	}
	link := filepath.Join(home, "link")
	if err = os.Symlink(path, link); err == nil && devinFileKey(link) != "" {
		t.Fatal("symlink read")
	}
}
