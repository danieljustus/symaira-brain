package usage

import (
	"bytes"
	"encoding/json"
	"time"
)

// parseClaudeKeychainBlob reads the subscription OAuth token out of the
// security command's JSON. MCP-only OAuth entries do not authenticate to the
// subscription usage endpoint and are deliberately ignored.
func parseClaudeKeychainBlob(blob []byte) (string, *time.Time, bool) {
	var root struct {
		ClaudeAIOAuth *struct {
			AccessToken string `json:"accessToken"`
			ExpiresAt   *int64 `json:"expiresAt"`
		} `json:"claudeAiOauth"`
	}
	if err := json.Unmarshal(bytes.TrimSpace(blob), &root); err != nil {
		return "", nil, false
	}
	if root.ClaudeAIOAuth == nil || root.ClaudeAIOAuth.AccessToken == "" {
		return "", nil, false
	}
	var expiresAt *time.Time
	if milliseconds := root.ClaudeAIOAuth.ExpiresAt; milliseconds != nil && *milliseconds > 0 {
		expiry := time.UnixMilli(*milliseconds).UTC()
		expiresAt = &expiry
	}
	return root.ClaudeAIOAuth.AccessToken, expiresAt, true
}
