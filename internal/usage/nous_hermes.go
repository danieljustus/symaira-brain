package usage

import (
	"encoding/json"
	"strings"
	"time"
)

// Hermes stores Nous OAuth state directly in auth.json, not in Keychain.
// Never reuse quarantined credentials or rotate its single-use refresh token.
func currentHermesNousToken(data []byte) (string, bool) {
	var root struct {
		Providers json.RawMessage              `json:"providers"`
		Pool      map[string][]hermesNousState `json:"credential_pool"`
	}
	if json.Unmarshal(data, &root) != nil {
		return "", false
	}
	var providers map[string]hermesNousState
	if len(root.Providers) > 0 && string(root.Providers) != "null" {
		if json.Unmarshal(root.Providers, &providers) != nil {
			return "", false
		}
	}
	state, exists := providers["nous"]
	if exists {
		return state.liveToken(), true
	}
	for _, entry := range root.Pool["nous"] {
		if token := entry.liveToken(); token != "" {
			return token, true
		}
	}
	return "", len(root.Providers) > 0 || root.Pool != nil
}

type hermesNousState struct {
	AccessToken   string          `json:"access_token"`
	InvokeJWT     string          `json:"invoke_jwt"`
	ExpiresAt     string          `json:"expires_at"`
	LastAuthError json.RawMessage `json:"last_auth_error"`
}

func (s hermesNousState) liveToken() string {
	if len(s.LastAuthError) > 0 && string(s.LastAuthError) != "null" {
		return ""
	}
	token := s.AccessToken
	if token == "" {
		token = s.InvokeJWT
	}
	if token == "" {
		return ""
	}
	if s.ExpiresAt != "" {
		expiry, err := time.Parse(time.RFC3339Nano, s.ExpiresAt)
		if err != nil || !expiry.After(time.Now()) {
			return ""
		}
	}
	if strings.Contains(token, ".") && !nousJWTIsLive(token) {
		return ""
	}
	return token
}
