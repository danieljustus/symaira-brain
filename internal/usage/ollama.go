package usage

import (
	"context"
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"errors"
	"net/http"
	"os"
	"path/filepath"
	"strconv"
	"time"
)

// OllamaProvider is explicitly selected by the caller, never credential-discovered.
// Construction performs no credential I/O; only Fetch reads the local signing key.
type OllamaProvider struct{ client *http.Client }

func NewOllamaProvider(client *http.Client) *OllamaProvider { return &OllamaProvider{client: client} }
func (*OllamaProvider) ID() string                          { return "ollama" }
func (*OllamaProvider) DisplayName() string                 { return "Ollama" }
func (*OllamaProvider) IsConfigured() bool                  { return true }
func (*OllamaProvider) AuthStatus() AuthStatus {
	return AuthStatus{Status: "partial", Detail: "Ollama Cloud explicitly enabled; signing key checked on refresh", Source: "file"}
}
func (p *OllamaProvider) Strategies() []Strategy { return []Strategy{p} }
func (*OllamaProvider) Source() string           { return "api" }
func (p *OllamaProvider) Fetch(ctx context.Context) (*UsageSnapshot, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return nil, ollamaKeyError()
	}
	data, err := readOllamaKeyFile(filepath.Join(home, ".ollama", "id_ed25519"))
	if err != nil {
		return nil, ollamaKeyError()
	}
	key, public, err := parseOllamaKey(data)
	if err != nil {
		return nil, err
	}
	req, err := ollamaRequest(ctx, key, public, time.Now().Unix())
	if err != nil {
		return nil, err
	}
	// Do not propagate transport errors that could include a signed header.
	body, err := FetchJSON[json.RawMessage](ctx, req, "ollama", p.client)
	if err != nil {
		var httpErr *HTTPError
		if errors.As(err, &httpErr) {
			return nil, &HTTPError{Kind: httpErr.Kind, Status: httpErr.Status}
		}
		return nil, err
	}
	return parseOllamaBalance(body, time.Now().UTC())
}

func ollamaRequest(ctx context.Context, key ed25519.PrivateKey, public []byte, timestamp int64) (*http.Request, error) {
	uri := "/api/balance?ts=" + strconv.FormatInt(timestamp, 10)
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, "https://ollama.com"+uri, nil)
	if err != nil {
		return nil, err
	}
	signature := ed25519.Sign(key, []byte("GET,"+uri))
	req.Header.Set("Authorization", base64.StdEncoding.EncodeToString(public)+":"+base64.StdEncoding.EncodeToString(signature))
	req.Header.Set("Accept", "application/json")
	return req, nil
}

func ollamaKeyError() error {
	return &PayloadError{ProviderID: "ollama", Detail: "unusable local Ollama signing key (requires an owned, private, unencrypted Ed25519 key)"}
}
