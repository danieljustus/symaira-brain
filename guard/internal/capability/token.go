// Package capability mints and verifies short-lived, purpose-bound
// capability tokens for headless symguard callers (scheduled agent runs,
// CI jobs, autonomous loops), replacing the status quo of handing every
// non-interactive caller a standing credential.
//
// The design adopts the pattern of the upstream AIcortex app/actas.py
// layer, not its code: each token carries {subject, purpose, scope, iat,
// exp, jti}, is signed with HMAC-SHA256, expires in minutes rather than
// hours, and must verify before any identity switch. Two details are
// copied as a pattern:
//
//  1. The signing key is HKDF-derived from existing key material under a
//     fixed domain-separation label (DeriveKey), never a raw key reused
//     across purposes.
//  2. Fail-closed key material: with no key, tokens can be neither issued
//     nor verified (ErrNoKeyMaterial) — delegation is simply unavailable,
//     never forgeable.
//
// symguard owns no long-lived secret today, so the key material is a
// guard-local random key generated at first run and stored under the XDG
// data directory (~/.local/share/symguard/capability.key, 0600 perms).
//
// Scope model: a token's scope is a CEILING. It is evaluated as an
// intersection with the identity's normal policy result
// (policy.ScopeCeiling), never a widening, and the control-plane
// deny-list in this package (DenyControlPlane, InScope) is unconditional —
// no token, not even one with scope "*", may reach symguard's own
// management operations.
//
// # Decision log (issue #91)
//
//   - Key material: guard-local random key generated at first run under
//     $XDG_DATA_HOME/symguard (fallback ~/.local/share/symguard),
//     0600 perms, never silently regenerated once present (regeneration
//     would invalidate every outstanding token). Tests inject key material
//     directly or via a temp directory.
//   - Both issue and verify live in-repo: issuing serves the CLI/agent
//     flow; verification is what the policy engine and future MCP proxy
//     use. No CLI wiring yet — the transport layer is out of scope for
//     this change.
//   - JTI uniqueness is not tracked (the issuer is stateless); the claim
//     exists so a future revocation/replay layer can key off it.
//   - Verify checks, in order: fail-closed key material, structure, HMAC
//     signature, claim invariants, then expiry — a tampered token is
//     rejected before any claim is trusted.
package capability

import (
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"time"
)

// DefaultTTLMinutes is the recommended token lifetime. Tokens are minted
// for minutes, not hours; five minutes matches the upstream pattern.
const DefaultTTLMinutes = 5

// ScopeAll is the wildcard scope entry: it grants every capability that is
// not on the control-plane deny-list. The deny-list still applies.
const ScopeAll = "*"

// Sentinel errors, following the AGENTS.md lowercase-after-colon style.
var (
	// ErrNoKeyMaterial means tokens can be neither issued nor verified
	// (fail closed): delegation is unavailable, never forgeable.
	ErrNoKeyMaterial = errors.New("capability: no key material")
	// ErrMalformed is returned when a token string cannot be parsed.
	ErrMalformed = errors.New("capability: malformed token")
	// ErrInvalidSignature is returned when the HMAC signature does not match.
	ErrInvalidSignature = errors.New("capability: invalid signature")
	// ErrExpired is returned when the token's expiry is in the past.
	ErrExpired = errors.New("capability: token expired")
	// ErrInvalidClaims is returned when claims fail structural validation.
	ErrInvalidClaims = errors.New("capability: invalid claims")
)

// Claims is the purpose-bound payload of a capability token. Field order
// is fixed: the HMAC signature covers the canonical JSON encoding of this
// struct, so marshaling must stay deterministic.
type Claims struct {
	Subject string   `json:"sub"`     // identity the token acts for
	Purpose string   `json:"purpose"` // bound purpose (job id, CI run, ...)
	Scope   []string `json:"scope"`   // granted capability names; ScopeAll = everything
	IAT     int64    `json:"iat"`     // issued-at, unix seconds
	Exp     int64    `json:"exp"`     // expiry, unix seconds
	JTI     string   `json:"jti"`     // unique token id (replay/revocation key)
}

// Token is a signed capability token: JSON claims plus an HMAC-SHA256
// signature over the canonical JSON encoding of those claims.
type Token struct {
	Claims    Claims `json:"claims"`
	Signature string `json:"sig"` // base64url HMAC-SHA256
}

// Encode serializes the token to its wire form (base64url JSON). It cannot
// fail: every field of Token is marshalable.
func (t Token) Encode() string {
	b, err := json.Marshal(t)
	if err != nil {
		return ""
	}
	return base64.RawURLEncoding.EncodeToString(b)
}

// Decode parses the wire form back into a Token.
func Decode(s string) (Token, error) {
	var t Token
	if s == "" {
		return t, ErrMalformed
	}
	raw, err := base64.RawURLEncoding.DecodeString(s)
	if err != nil {
		return t, fmt.Errorf("capability: decode: %w", ErrMalformed)
	}
	if err := json.Unmarshal(raw, &t); err != nil {
		return t, fmt.Errorf("capability: decode: %w", ErrMalformed)
	}
	return t, nil
}

// Issuer mints signed capability tokens from key material. A keyless
// Issuer fails closed: every Issue returns ErrNoKeyMaterial.
type Issuer struct {
	key []byte // derived signing key; nil means fail-closed
}

// NewIssuer creates an issuer from master key material. The signing key is
// HKDF-derived (see DeriveKey); empty or too-short material yields an
// issuer that always fails closed.
func NewIssuer(master []byte) *Issuer {
	key, err := DeriveKey(master)
	if err != nil {
		return &Issuer{} // fail-closed: no signing possible
	}
	return &Issuer{key: key}
}

// Issue mints and signs a purpose-bound token for subject with the given
// scope, valid for ttlMinutes minutes. The expiry is always computed by
// the issuer — callers cannot mint long-lived tokens.
func (i *Issuer) Issue(subject, purpose string, scope []string, ttlMinutes int) (string, error) {
	if len(i.key) == 0 {
		return "", ErrNoKeyMaterial
	}
	if subject == "" {
		return "", fmt.Errorf("capability: issue: %w: empty subject", ErrInvalidClaims)
	}
	if purpose == "" {
		return "", fmt.Errorf("capability: issue: %w: empty purpose", ErrInvalidClaims)
	}
	if ttlMinutes <= 0 {
		return "", fmt.Errorf("capability: issue: %w: ttl must be positive minutes", ErrInvalidClaims)
	}
	if err := validateScope(scope); err != nil {
		return "", fmt.Errorf("capability: issue: %w", err)
	}
	jti, err := newJTI()
	if err != nil {
		return "", err
	}
	now := time.Now().Unix()
	claims := Claims{
		Subject: subject,
		Purpose: purpose,
		Scope:   append([]string(nil), scope...), // copy: caller mutation must not leak in
		IAT:     now,
		Exp:     now + int64(ttlMinutes)*60,
		JTI:     jti,
	}
	tok, err := sign(i.key, claims)
	if err != nil {
		return "", err
	}
	return tok.Encode(), nil
}

// Verifier validates signed capability tokens against key material. A
// keyless Verifier fails closed: every Verify returns ErrNoKeyMaterial.
type Verifier struct {
	key []byte // derived signing key; nil means fail-closed
}

// NewVerifier creates a verifier from master key material; empty or
// too-short material yields a fail-closed verifier.
func NewVerifier(master []byte) *Verifier {
	key, err := DeriveKey(master)
	if err != nil {
		return &Verifier{}
	}
	return &Verifier{key: key}
}

// Verify checks the token's structure, signature, claim invariants, and
// expiry, and returns the claims when the token is acceptable. Order
// matters: a token with a bad signature is rejected before any claim is
// trusted, and an expired-but-genuine token reports ErrExpired.
func (v *Verifier) Verify(encoded string) (Claims, error) {
	var zero Claims
	if len(v.key) == 0 {
		return zero, ErrNoKeyMaterial
	}
	tok, err := Decode(encoded)
	if err != nil {
		return zero, err
	}
	payload, err := json.Marshal(tok.Claims)
	if err != nil {
		return zero, fmt.Errorf("capability: verify: %w", err)
	}
	mac := hmac.New(sha256.New, v.key)
	_, _ = mac.Write(payload)
	sig, err := base64.RawURLEncoding.DecodeString(tok.Signature)
	if err != nil || !hmac.Equal(mac.Sum(nil), sig) {
		return zero, ErrInvalidSignature
	}
	if err := validateClaims(tok.Claims); err != nil {
		return zero, err
	}
	if tok.Claims.Exp <= time.Now().Unix() {
		return zero, ErrExpired
	}
	return tok.Claims, nil
}

// sign signs claims with the derived key, returning the signed token.
func sign(key []byte, c Claims) (Token, error) {
	payload, err := json.Marshal(c)
	if err != nil {
		return Token{}, fmt.Errorf("capability: sign: %w", err)
	}
	mac := hmac.New(sha256.New, key)
	_, _ = mac.Write(payload)
	sig := base64.RawURLEncoding.EncodeToString(mac.Sum(nil))
	return Token{Claims: c, Signature: sig}, nil
}

// validateClaims checks structural claim invariants shared by issue and
// verify.
func validateClaims(c Claims) error {
	if c.Subject == "" || c.Purpose == "" || c.JTI == "" {
		return fmt.Errorf("capability: %w: subject, purpose, and jti must be non-empty", ErrInvalidClaims)
	}
	if c.IAT <= 0 {
		return fmt.Errorf("capability: %w: iat must be positive", ErrInvalidClaims)
	}
	if c.Exp <= c.IAT {
		return fmt.Errorf("capability: %w: exp must be after iat", ErrInvalidClaims)
	}
	return validateScope(c.Scope)
}

// validateScope checks that every scope entry is a non-empty string and
// that the wildcard appears at most once.
func validateScope(scope []string) error {
	seenWild := false
	for _, s := range scope {
		if s == "" {
			return fmt.Errorf("capability: %w: empty scope entry", ErrInvalidClaims)
		}
		if s == ScopeAll {
			if seenWild {
				return fmt.Errorf("capability: %w: duplicate wildcard scope", ErrInvalidClaims)
			}
			seenWild = true
		}
	}
	return nil
}

// newJTI returns a random 16-byte hex token id.
func newJTI() (string, error) {
	b := make([]byte, 16)
	if _, err := rand.Read(b); err != nil {
		return "", fmt.Errorf("capability: jti: %w", err)
	}
	return hex.EncodeToString(b), nil
}
