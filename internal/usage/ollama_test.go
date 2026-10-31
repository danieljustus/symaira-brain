package usage

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"encoding/base64"
	"encoding/pem"
	"io"
	"net/http"
	"strings"
	"testing"
	"time"

	"golang.org/x/crypto/ssh"
)

func ollamaFixture(t *testing.T) (ed25519.PrivateKey, []byte) {
	t.Helper()
	key := ed25519.NewKeyFromSeed(bytes.Repeat([]byte{42}, 32)) // generated test-only material
	block, err := ssh.MarshalPrivateKey(key, "offline Ollama fixture")
	if err != nil {
		t.Fatal(err)
	}
	return key, pem.EncodeToMemory(block)
}

func TestOllamaSigningAndStrictContainer(t *testing.T) {
	key, data := ollamaFixture(t)
	encrypted, err := ssh.MarshalPrivateKeyWithPassphrase(key, "encrypted test-only fixture", []byte("fixture passphrase"))
	if err != nil {
		t.Fatal(err)
	}
	if _, _, err := parseOllamaKey(pem.EncodeToMemory(encrypted)); err == nil {
		t.Fatal("encrypted key accepted")
	}
	parsed, public, err := parseOllamaKey(data)
	if err != nil || !bytes.Equal(key, parsed) {
		t.Fatalf("valid generated key: %v", err)
	}
	req, err := ollamaRequest(context.Background(), parsed, public, 1234567890)
	if err != nil {
		t.Fatal(err)
	}
	if req.URL.String() != "https://ollama.com/api/balance?ts=1234567890" || req.Method != "GET" {
		t.Fatal("wrong request")
	}
	parts := strings.Split(req.Header.Get("Authorization"), ":")
	if len(parts) != 2 || parts[0] != base64.StdEncoding.EncodeToString(public) {
		t.Fatal("auth format")
	}
	signature, err := base64.StdEncoding.DecodeString(parts[1])
	if err != nil {
		t.Fatal(err)
	}
	if len(signature) != 64 || !ed25519.Verify(key.Public().(ed25519.PublicKey), []byte("GET,"+req.URL.RequestURI()), signature) {
		t.Fatal("signature invalid")
	}
	for _, challenge := range []string{"POST," + req.URL.RequestURI(), "GET,/api/balance?ts=1234567891", "GET,https://ollama.com" + req.URL.RequestURI()} {
		if ed25519.Verify(key.Public().(ed25519.PublicKey), []byte(challenge), signature) {
			t.Fatal("tampered signature accepted")
		}
	}
	block, _ := pem.Decode(data)
	for _, kind := range []string{"outer-public", "repeated-public", "private-public", "checkint", "padding", "cipher", "trailing"} {
		t.Run(kind, func(t *testing.T) {
			blob := bytes.Clone(block.Bytes)
			positions := []int{}
			offset := 0
			for {
				i := bytes.Index(blob[offset:], key[32:])
				if i < 0 {
					break
				}
				positions = append(positions, offset+i)
				offset += i + 32
			}
			if len(positions) != 3 {
				t.Fatal("fixture public positions", positions)
			}
			switch kind {
			case "outer-public":
				blob[positions[0]] ^= 1
			case "repeated-public":
				blob[positions[1]] ^= 1
			case "private-public":
				blob[positions[2]] ^= 1
			case "checkint":
				blob[positions[0]+32+4] ^= 1
			case "padding":
				blob[len(blob)-1] = 0
			case "cipher":
				i := bytes.Index(blob, []byte("none"))
				copy(blob[i:i+4], "evil")
			case "trailing":
				blob = append(blob, 0)
			}
			mutated := pem.EncodeToMemory(&pem.Block{Type: "OPENSSH PRIVATE KEY", Bytes: blob})
			if _, _, err := parseOllamaKey(mutated); err == nil {
				t.Fatal("corrupt key accepted")
			}
		})
	}
	for i := 0; i < len(bytes.TrimSpace(data)); i++ {
		if _, _, err := parseOllamaKey(data[:i]); err == nil {
			t.Fatalf("truncated PEM %d accepted", i)
		}
	}
}

func TestOllamaBalance(t *testing.T) {
	for _, test := range []struct {
		name, body, used, limit, balance string
		count                            int
		invalid                          bool
	}{
		{name: "monthly", body: `{"included":{"balance_usd":72.5,"allowance_usd":100,"period":{"until":"2026-10-15T09:30:00Z"}},"purchased":{"balance_usd":25}}`, used: "27.5", limit: "100", balance: "25", count: 1},
		{name: "legacy", body: `{"included":{"session":{"remaining_percent":75},"weekly":{"remaining_percent":"40"}}}`, used: "25", limit: "100", count: 2},
		{name: "zero", body: `{"included":{"balance_usd":100,"allowance_usd":100},"purchased":{"balance_usd":0}}`, used: "0", limit: "100", balance: "0", count: 1},
		{name: "clamp", body: `{"included":{"session":{"remaining_percent":200}}}`, used: "0", limit: "100", count: 1},
		{name: "missing", body: `{"included":{},"purchased":{"balance_usd":25}}`, invalid: true},
		{name: "nonfinite", body: `{"included":{"session":{"remaining_percent":"NaN"}}}`, invalid: true},
		{name: "invalid", body: `[]`, invalid: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			snap, err := parseOllamaBalance([]byte(test.body), time.Unix(0, 0).UTC())
			if test.invalid {
				if err == nil {
					t.Fatal("expected no-data error")
				}
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			if len(snap.Meters) != test.count || *snap.Meters[0].Used != test.used || *snap.Meters[0].Limit != test.limit {
				t.Fatalf("meters: %+v", snap.Meters)
			}
			if test.balance != "" {
				if snap.Balance == nil || *snap.Balance != test.balance {
					t.Fatal("balance")
				}
			} else if snap.Balance != nil {
				t.Fatal("invented balance")
			}
			if test.name == "monthly" && snap.Meters[0].ResetsAt == nil {
				t.Fatal("missing reset")
			}
		})
	}
}

type ollamaRoundTrip func(*http.Request) (*http.Response, error)

func (f ollamaRoundTrip) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }
func ollamaResponse(status int, body string) *http.Response {
	return &http.Response{StatusCode: status, Header: http.Header{}, Body: io.NopCloser(strings.NewReader(body))}
}
