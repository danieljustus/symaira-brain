package usage

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"
)

func TestZAIQuota(t *testing.T) {
	cases := []struct {
		name, body string
		count      int
		bad        bool
	}{
		{"absent", `{}`, 0, true}, {"empty", `{"limits":[]}`, 0, false},
		{"zero", `{"data":{"limits":[{"type":"CREDIT_LIMIT","unit":3,"number":5,"percentage":0,"nextResetTime":1770648402389},{"type":"TOKENS_LIMIT","unit":6,"number":1,"percentage":40},{"type":"TIME_LIMIT","currentValue":0,"usage":0}]}}`, 3, false},
		{"missing", `{"limits":[{"type":"CREDIT_LIMIT","unit":3,"number":5}]}`, 0, true},
		{"malformed", `{"limits":[{"type":"CREDIT_LIMIT","unit":3,"number":5,"percentage":"bad"}]}`, 0, true},
		{"bool", `{"limits":[{"type":"TIME_LIMIT","currentValue":false,"usage":3}]}`, 0, true},
		{"business", `{"success":false,"limits":[]}`, 0, true},
		{"containerDoesNotInherit", `{"data":{},"limits":[]}`, 0, true},
		{"nullEntry", `{"limits":[null]}`, 0, true},
		{"durationSession", `{"limits":[{"name":"CREDIT_LIMIT","unit":4,"number":0.5,"percentage":25}]}`, 1, false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			var root map[string]json.RawMessage
			if err := json.Unmarshal([]byte(tc.body), &root); err != nil {
				t.Fatal(err)
			}
			s, err := parseZAIQuota(root)
			if (err != nil) != tc.bad {
				t.Fatalf("snapshot=%v err=%v", s, err)
			}
			if err == nil && len(s.Meters) != tc.count {
				t.Fatal(s)
			}
			if tc.name == "zero" {
				if *s.Meters[0].Used != "0" || s.Meters[0].ResetsAt.UnixMilli() != 1770648402389 {
					t.Fatal(s)
				}
			}
		})
	}
}
func TestZAIRequestAndAuth(t *testing.T) {
	t.Setenv("ZAI_API_KEY", "")
	if NewZAIProvider(nil).IsConfigured() {
		t.Fatal("missing credential configured")
	}
	old := vaultResolver
	t.Cleanup(func() { vaultResolver = old })
	vaultResolver = func(ref, fallback string) (string, error) {
		if ref != "symvault://test/zai" {
			t.Fatal(ref)
		}
		return "hermetic-key", nil
	}
	t.Setenv("ZAI_API_KEY", "symvault://test/zai")
	c, tr := fakeClient(200, []byte(`{"limits":[]}`), nil)
	p := NewZAIProvider(c)
	if !p.IsConfigured() || p.AuthStatus().Source != "vault" {
		t.Fatal(p.AuthStatus())
	}
	if _, err := p.Strategies()[0].Fetch(context.Background()); err != nil {
		t.Fatal(err)
	}
	r := tr.lastRequest
	if r.Method != "GET" || r.URL.String() != "https://api.z.ai/api/monitor/usage/quota/limit" || r.Header.Get("Authorization") != "Bearer hermetic-key" || r.Body != nil {
		t.Fatal("request contract")
	}
	vaultResolver = func(string, string) (string, error) { return "", errors.New("hermetic-key") }
	p = NewZAIProvider(nil)
	if p.IsConfigured() || strings.Contains(p.AuthStatus().Detail, "hermetic-key") {
		t.Fatal("credential error leaked")
	}
	c, _ = fakeErrorClient(errors.New("hermetic-key"))
	t.Setenv("ZAI_API_KEY", "hermetic-key")
	p = NewZAIProvider(c)
	_, err := p.Strategies()[0].Fetch(context.Background())
	if err == nil || strings.Contains(err.Error(), "hermetic-key") {
		t.Fatal("transport error leaked")
	}
}
