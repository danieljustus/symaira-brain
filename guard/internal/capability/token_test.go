package capability

import (
	"errors"
	"strings"
	"testing"
	"time"
)

// testKey returns fresh 32-byte key material for a test.
func testKey(t *testing.T) []byte {
	t.Helper()
	key, err := GenerateKey()
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	return key
}

func TestIssueVerify_RoundTrip(t *testing.T) {
	key := testKey(t)
	issuer := NewIssuer(key)
	verifier := NewVerifier(key)

	encoded, err := issuer.Issue("build-bot", "job-4821", []string{"read_public", "read_private"}, DefaultTTLMinutes)
	if err != nil {
		t.Fatalf("Issue() error = %v", err)
	}
	claims, err := verifier.Verify(encoded)
	if err != nil {
		t.Fatalf("Verify() error = %v", err)
	}
	if claims.Subject != "build-bot" {
		t.Errorf("Subject = %q, want build-bot", claims.Subject)
	}
	if claims.Purpose != "job-4821" {
		t.Errorf("Purpose = %q, want job-4821", claims.Purpose)
	}
	if len(claims.Scope) != 2 || claims.Scope[0] != "read_public" || claims.Scope[1] != "read_private" {
		t.Errorf("Scope = %v, want [read_public read_private]", claims.Scope)
	}
	if claims.IAT <= 0 {
		t.Errorf("IAT = %d, want > 0", claims.IAT)
	}
	if got := claims.Exp - claims.IAT; got != int64(DefaultTTLMinutes*60) {
		t.Errorf("TTL = %ds, want %ds", got, DefaultTTLMinutes*60)
	}
	if claims.JTI == "" {
		t.Error("JTI = empty, want non-empty")
	}
}

func TestVerify_Errors(t *testing.T) {
	key := testKey(t)
	otherKey := testKey(t)
	issuer := NewIssuer(key)
	good, err := issuer.Issue("build-bot", "job-9", []string{"shell"}, DefaultTTLMinutes)
	if err != nil {
		t.Fatalf("Issue() error = %v", err)
	}

	// Expired token: valid signature, expiry in the past.
	expiredClaims := Claims{
		Subject: "build-bot",
		Purpose: "job-9",
		Scope:   []string{"shell"},
		IAT:     time.Now().Add(-2 * time.Hour).Unix(),
		Exp:     time.Now().Add(-1 * time.Hour).Unix(),
		JTI:     "expired-jti",
	}
	expiredTok, err := sign(issuer.key, expiredClaims)
	if err != nil {
		t.Fatalf("sign() error = %v", err)
	}

	tests := []struct {
		name    string
		key     []byte
		token   string
		wantErr error
	}{
		{"valid token verifies", key, good, nil},
		{"wrong key rejected", otherKey, good, ErrInvalidSignature},
		{"no key material", nil, good, ErrNoKeyMaterial},
		{"empty token", key, "", ErrMalformed},
		{"garbage token", key, "not-a-token", ErrMalformed},
		{"non-base64 token", key, "%%%%", ErrMalformed},
		{"expired token", key, expiredTok.Encode(), ErrExpired},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			v := NewVerifier(tt.key)
			_, err := v.Verify(tt.token)
			if tt.wantErr == nil {
				if err != nil {
					t.Fatalf("Verify() error = %v, want nil", err)
				}
				return
			}
			if !errors.Is(err, tt.wantErr) {
				t.Fatalf("Verify() error = %v, want %v", err, tt.wantErr)
			}
		})
	}
}

func TestVerify_TamperedToken(t *testing.T) {
	key := testKey(t)
	issuer := NewIssuer(key)
	encoded, err := issuer.Issue("build-bot", "job-7", []string{"shell"}, DefaultTTLMinutes)
	if err != nil {
		t.Fatalf("Issue() error = %v", err)
	}
	tok, err := Decode(encoded)
	if err != nil {
		t.Fatalf("Decode() error = %v", err)
	}

	// Claim tamper: subject changed without re-signing.
	tamperedClaims := tok.Claims
	tamperedClaims.Subject = "attacker"
	tampered := Token{Claims: tamperedClaims, Signature: tok.Signature}
	// Signature tamper: claims intact, signature corrupted.
	corrupted := Token{Claims: tok.Claims, Signature: strings.Repeat("A", len(tok.Signature))}

	tests := []struct {
		name    string
		token   Token
		wantErr error
	}{
		{"claim tamper", tampered, ErrInvalidSignature},
		{"signature tamper", corrupted, ErrInvalidSignature},
		{"missing signature", Token{Claims: tok.Claims}, ErrInvalidSignature},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			_, err := NewVerifier(key).Verify(tt.token.Encode())
			if !errors.Is(err, tt.wantErr) {
				t.Fatalf("Verify() error = %v, want %v", err, tt.wantErr)
			}
		})
	}
}

func TestVerify_InvalidClaims(t *testing.T) {
	key := testKey(t)
	base := Claims{
		Subject: "build-bot",
		Purpose: "job-1",
		Scope:   []string{"shell"},
		IAT:     time.Now().Add(-time.Minute).Unix(),
		Exp:     time.Now().Add(time.Minute).Unix(),
		JTI:     "jti-1",
	}
	tests := []struct {
		name   string
		mutate func(*Claims)
	}{
		{"empty subject", func(c *Claims) { c.Subject = "" }},
		{"empty purpose", func(c *Claims) { c.Purpose = "" }},
		{"empty jti", func(c *Claims) { c.JTI = "" }},
		{"zero iat", func(c *Claims) { c.IAT = 0 }},
		{"exp before iat", func(c *Claims) {
			c.IAT = time.Now().Add(time.Hour).Unix()
			c.Exp = time.Now().Unix()
		}},
		{"empty scope entry", func(c *Claims) { c.Scope = []string{""} }},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			c := base
			tt.mutate(&c)
			derived, err := DeriveKey(key)
			if err != nil {
				t.Fatalf("DeriveKey() error = %v", err)
			}
			tok, err := sign(derived, c)
			if err != nil {
				t.Fatalf("sign() error = %v", err)
			}
			_, err = NewVerifier(key).Verify(tok.Encode())
			if !errors.Is(err, ErrInvalidClaims) {
				t.Fatalf("Verify() error = %v, want %v", err, ErrInvalidClaims)
			}
		})
	}
}

func TestIssue_InvalidInputs(t *testing.T) {
	key := testKey(t)
	issuer := NewIssuer(key)
	tests := []struct {
		name       string
		subject    string
		purpose    string
		scope      []string
		ttlMinutes int
		wantErr    error
	}{
		{"empty subject", "", "job-1", []string{"shell"}, DefaultTTLMinutes, ErrInvalidClaims},
		{"empty purpose", "build-bot", "", []string{"shell"}, DefaultTTLMinutes, ErrInvalidClaims},
		{"zero ttl", "build-bot", "job-1", []string{"shell"}, 0, ErrInvalidClaims},
		{"negative ttl", "build-bot", "job-1", []string{"shell"}, -5, ErrInvalidClaims},
		{"empty scope entry", "build-bot", "job-1", []string{""}, DefaultTTLMinutes, ErrInvalidClaims},
		{"duplicate wildcard", "build-bot", "job-1", []string{"*", "*"}, DefaultTTLMinutes, ErrInvalidClaims},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			_, err := issuer.Issue(tt.subject, tt.purpose, tt.scope, tt.ttlMinutes)
			if !errors.Is(err, tt.wantErr) {
				t.Fatalf("Issue() error = %v, want %v", err, tt.wantErr)
			}
		})
	}
}

func TestFailClosed_NoKeyMaterial(t *testing.T) {
	if _, err := NewIssuer(nil).Issue("build-bot", "job-1", []string{"shell"}, DefaultTTLMinutes); !errors.Is(err, ErrNoKeyMaterial) {
		t.Fatalf("Issue() with nil key error = %v, want %v", err, ErrNoKeyMaterial)
	}
	if _, err := NewIssuer([]byte("too-short")).Issue("build-bot", "job-1", []string{"shell"}, DefaultTTLMinutes); !errors.Is(err, ErrNoKeyMaterial) {
		t.Fatalf("Issue() with short key error = %v, want %v", err, ErrNoKeyMaterial)
	}
	if _, err := NewVerifier(nil).Verify("any-token"); !errors.Is(err, ErrNoKeyMaterial) {
		t.Fatalf("Verify() with nil key error = %v, want %v", err, ErrNoKeyMaterial)
	}
}

func TestIssue_ScopeIsCopied(t *testing.T) {
	key := testKey(t)
	scope := []string{"read_public"}
	encoded, err := NewIssuer(key).Issue("build-bot", "job-1", scope, DefaultTTLMinutes)
	if err != nil {
		t.Fatalf("Issue() error = %v", err)
	}
	scope[0] = "shell" // caller mutation must not affect the issued token
	claims, err := NewVerifier(key).Verify(encoded)
	if err != nil {
		t.Fatalf("Verify() error = %v", err)
	}
	if len(claims.Scope) != 1 || claims.Scope[0] != "read_public" {
		t.Errorf("Scope = %v, want [read_public]", claims.Scope)
	}
}

func TestToken_EncodeDecode(t *testing.T) {
	tok := Token{
		Claims:    Claims{Subject: "s", Purpose: "p", Scope: []string{"shell"}, IAT: 1, Exp: 2, JTI: "j"},
		Signature: "sig",
	}
	got, err := Decode(tok.Encode())
	if err != nil {
		t.Fatalf("Decode() error = %v", err)
	}
	if got.Claims.Subject != "s" || got.Signature != "sig" {
		t.Errorf("round trip mismatch: %+v", got)
	}
	for _, s := range []string{"", "%%%", "not-base64!!"} {
		if _, err := Decode(s); !errors.Is(err, ErrMalformed) {
			t.Errorf("Decode(%q) error = %v, want %v", s, err, ErrMalformed)
		}
	}
}
