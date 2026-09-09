package capability

import (
	"encoding/base64"
	"encoding/hex"
	"fmt"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/guard/internal/model"
	"github.com/danieljustus/symaira-brain/guard/internal/policy"
)

func capabilityCases(t *testing.T, now int64) []capabilityCase {
	t.Helper()
	master := make([]byte, KeySize)
	for i := range master {
		master[i] = byte(i)
	}
	key, err := DeriveKey(master)
	if err != nil {
		t.Fatal(err)
	}
	masterHex := hex.EncodeToString(master)
	base := Claims{Subject: "build-bot", Purpose: "job-4821", Scope: []string{"read_public", "shell"}, IAT: now - 1, Exp: now + 300, JTI: "00112233445566778899aabbccddeeff"}
	signed := func(c Claims) Token {
		token, err := sign(key, c)
		if err != nil {
			t.Fatal(err)
		}
		return token
	}
	var cases []capabilityCase
	add := func(c capabilityCase) {
		c.Now = now
		cases = append(cases, c)
	}
	for _, size := range []int{0, 1, 31, 32, 64} {
		add(capabilityCase{ID: fmt.Sprintf("derive-%d", size), Kind: "derive", MasterHex: strings.Repeat("ab", size)})
	}
	for _, change := range []struct {
		id     string
		mutate func(*Claims)
	}{
		{"valid", func(*Claims) {}},
		{"html-unicode", func(c *Claims) { c.Subject = "<bot>&\u2028\u2029é😀\n\t\x00"; c.Purpose = `quote"slash\` }},
		{"nil-scope", func(c *Claims) { c.Scope = nil }},
		{"empty-scope", func(c *Claims) { c.Scope = []string{} }},
		{"wildcard", func(c *Claims) { c.Scope = []string{"*"} }},
		{"duplicate-regular", func(c *Claims) { c.Scope = []string{"shell", "shell"} }},
		{"explicit-control-plane", func(c *Claims) { c.Scope = []string{"symguard:config:set"} }},
		{"expired", func(c *Claims) { c.IAT = now - 2; c.Exp = now - 1 }},
		{"expiry-equal-now", func(c *Claims) { c.Exp = now }},
		{"expiry-one-second", func(c *Claims) { c.Exp = now + 1 }},
		{"future-iat-accepted", func(c *Claims) { c.IAT = now + 60; c.Exp = now + 120 }},
		{"empty-subject", func(c *Claims) { c.Subject = "" }},
		{"empty-purpose", func(c *Claims) { c.Purpose = "" }},
		{"empty-jti", func(c *Claims) { c.JTI = "" }},
		{"zero-iat", func(c *Claims) { c.IAT = 0 }},
		{"negative-iat", func(c *Claims) { c.IAT = -1 }},
		{"exp-before-iat", func(c *Claims) { c.Exp = c.IAT - 1 }},
		{"exp-equal-iat", func(c *Claims) { c.Exp = c.IAT }},
		{"empty-scope-entry", func(c *Claims) { c.Scope = []string{""} }},
		{"duplicate-wildcard", func(c *Claims) { c.Scope = []string{"*", "shell", "*"} }},
		{"claims-before-expiry", func(c *Claims) { c.Subject = ""; c.Exp = now - 1; c.IAT = now - 2 }},
		{"max-int64-expiry", func(c *Claims) { c.Exp = 1<<63 - 1 }},
	} {
		claims := base
		change.mutate(&claims)
		token := signed(claims)
		add(capabilityCase{ID: "sign-" + change.id, Kind: "sign", MasterHex: masterHex, Claims: &claims})
		add(capabilityCase{ID: "verify-" + change.id, Kind: "verify", MasterHex: masterHex, Token: token.Encode()})
	}
	good := signed(base)
	for _, size := range []int{0, 1, 31} {
		add(capabilityCase{ID: fmt.Sprintf("verify-key-priority-%d", size), Kind: "verify", MasterHex: strings.Repeat("ab", size), Token: "%%%"})
	}
	add(capabilityCase{ID: "verify-wrong-key", Kind: "verify", MasterHex: strings.Repeat("ab", 32), Token: good.Encode()})
	for _, change := range []struct {
		id     string
		mutate func(*Token)
	}{
		{"claim-tamper", func(token *Token) { token.Claims.Subject = "attacker" }},
		{"signature-tamper", func(token *Token) { token.Signature = strings.Repeat("A", len(token.Signature)) }},
		{"missing-signature", func(token *Token) { token.Signature = "" }},
		{"malformed-signature", func(token *Token) { token.Signature = "%%%" }},
		{"signature-padding", func(token *Token) { token.Signature += "=" }},
		{"signature-crlf", func(token *Token) { token.Signature = "\r\n" + token.Signature + "\n" }},
		{"signature-before-claims", func(token *Token) { token.Claims.Subject = ""; token.Claims.Exp = now - 1 }},
	} {
		token := good
		change.mutate(&token)
		add(capabilityCase{ID: "verify-" + change.id, Kind: "verify", MasterHex: masterHex, Token: token.Encode()})
	}
	for _, wire := range []struct{ id, token string }{
		{"empty", ""}, {"garbage", "not-a-token"}, {"non-base64", "%%%"},
		{"padding", good.Encode() + "="}, {"crlf", "\r\n" + good.Encode() + "\n"},
		{"whitespace", " " + good.Encode()},
	} {
		for _, kind := range []string{"decode", "verify"} {
			add(capabilityCase{ID: kind + "-wire-" + wire.id, Kind: kind, MasterHex: masterHex, Token: wire.token})
		}
	}
	for _, raw := range []struct{ id, json string }{
		{"null", "null"}, {"empty-object", "{}"}, {"array", "[]"}, {"number", "1"},
		{"null-fields", `{"claims":null,"sig":null}`},
		{"unknown", `{"unknown":{"future":true}}`},
		{"claims-array", `{"claims":[]}`}, {"sig-number", `{"sig":42}`},
		{"sub-number", `{"claims":{"sub":42}}`},
		{"iat-float", `{"claims":{"iat":1.0}}`}, {"iat-exponent", `{"claims":{"iat":1e2}}`},
		{"negative-zero", `{"claims":{"iat":-0,"exp":-0}}`},
		{"iat-overflow", `{"claims":{"iat":9223372036854775808}}`},
		{"scope-null-item", `{"claims":{"scope":[null]}}`},
		{"duplicate-scope-null", `{"claims":{"scope":["shell"],"scope":[null]}}`},
		{"scope-shrink-regrow", `{"claims":{"scope":["shell","network"],"scope":[null],"scope":[null,null]}}`},
		{"scope-empty-resets", `{"claims":{"scope":["shell"],"scope":[],"scope":[null]}}`},
		{"scope-null-resets", `{"claims":{"scope":["shell"],"scope":null,"scope":[null]}}`},
		{"duplicate-claims-scope", `{"claims":{"scope":["shell"]},"claims":{"scope":[null]}}`},
		{"scope-number", `{"claims":{"scope":[42]}}`},
		{"trailing-json", `{} {}`}, {"broken", `{"claims":`},
		{"duplicate-merge", `{"claims":{"sub":"s"},"claims":{"purpose":"p"},"sig":"s","sig":null}`},
		{"case-fold", `{"CLAIMS":{"SUB":"s","PURPOSE":"p","ſCOPE":[]},"SIG":"s"}`},
		{"unpaired-high-surrogate", `{"claims":{"sub":"\ud800"}}`},
		{"unpaired-low-surrogate", `{"claims":{"sub":"\udc00"}}`},
		{"surrogate-pair", `{"claims":{"sub":"\ud83d\ude00"}}`},
		{"literal-surrogate-escape", `{"claims":{"sub":"\\ud800"}}`},
		{"invalid-utf8", "{\"claims\":{\"sub\":\"\xe2\x82\"}}"},
		{"reordered-valid", `{"sig":` + oracleJSON(t, good.Signature) + `,"unknown":true,"claims":` + oracleJSON(t, good.Claims) + `}`},
	} {
		wire := base64.RawURLEncoding.EncodeToString([]byte(raw.json))
		for _, kind := range []string{"decode", "verify"} {
			add(capabilityCase{ID: kind + "-json-" + raw.id, Kind: kind, MasterHex: masterHex, Token: wire})
		}
	}
	// Noncanonical trailing bits are accepted by Go's non-Strict raw decoder.
	for _, kind := range []string{"decode", "verify"} {
		add(capabilityCase{ID: kind + "-noncanonical-base64", Kind: kind, MasterHex: masterHex, Token: "e31"})
	}
	cases = append(cases, capabilityScopeCases(now)...)
	return cases
}

func capabilityScopeCases(now int64) []capabilityCase {
	var cases []capabilityCase
	targets := []string{"shell", "network", "", "shell:sub", "symguard:config", "symguard:shell", "Symguard:config:set"}
	for _, prefix := range ControlPlanePrefixes {
		targets = append(targets, prefix, prefix+"set")
	}
	for i, target := range targets {
		for j, scope := range [][]string{nil, {"*"}, {target}, {"shell*"}} {
			cases = append(cases, capabilityCase{ID: fmt.Sprintf("scope-%02d-%d", i, j), Kind: "scope", Scope: scope, Target: target, Now: now})
		}
	}
	for _, decision := range []model.Decision{model.DecisionAllow, model.DecisionAsk, model.DecisionDeny, model.DecisionRequire, model.DecisionRedact, model.DecisionReadOnly, model.DecisionSandbox} {
		result := policy.Result{Decision: decision, Reason: "identity-policy", Matched: true, Precedence: 7}
		for i, entry := range []struct {
			scope  []string
			target string
		}{
			{nil, "shell"}, {[]string{"shell"}, "shell"}, {[]string{"network"}, "shell"},
			{[]string{"*"}, "shell"}, {[]string{"*"}, ""}, {[]string{"*"}, "symguard:config:set"},
		} {
			cases = append(cases, capabilityCase{ID: fmt.Sprintf("ceiling-%s-%d", decision, i), Kind: "ceiling", Scope: entry.scope, Target: entry.target, Result: &result, Now: now})
		}
	}
	return cases
}
