package daemon

import (
	"encoding/json"
	"regexp"
	"strings"
)

var (
	secretTextPattern  = regexp.MustCompile(`(?i)(password|passwd|pass|secret|token|api[_-]?key|apikey|access[_-]?key|authorization|auth|cookie|set-cookie|client[_-]?secret|private[_-]?key|encryption[_-]?key|credential|credentials)(\s*[:=]\s*)(Bearer\s+[^\s,}\]]+|"[^"]*"|'[^']*'|[^\s,}\]]+)`)
	urlPasswordPattern = regexp.MustCompile(`(?i)(\b[a-z][a-z0-9+.-]*://[^:/@\s]+:)[^@/\s]+(@)`)
)

// RedactDiagnostic removes common key/value credentials and URL userinfo from
// error and diagnostic strings before they cross CLI, daemon or MCP boundaries.
func RedactDiagnostic(text string) string {
	text = secretTextPattern.ReplaceAllString(text, "$1$2[REDACTED]")
	return urlPasswordPattern.ReplaceAllString(text, "$1[REDACTED]$2")
}

// RedactValue recursively masks credential-shaped fields and strings.
func RedactValue(value any) any {
	switch item := value.(type) {
	case map[string]any:
		redacted := make(map[string]any, len(item))
		for key, nested := range item {
			if secretFieldName(key) {
				redacted[key] = "[REDACTED]"
			} else {
				redacted[key] = RedactValue(nested)
			}
		}
		return redacted
	case []any:
		redacted := make([]any, len(item))
		for index, nested := range item {
			redacted[index] = RedactValue(nested)
		}
		return redacted
	case string:
		return RedactDiagnostic(item)
	default:
		return value
	}
}

func secretFieldName(key string) bool {
	normalized := strings.NewReplacer("_", "", "-", "").Replace(strings.ToLower(key))
	for _, secret := range []string{
		"password", "passwd", "pass", "secret", "token", "apikey", "accesskey", "auth",
		"cookie", "clientsecret", "privatekey", "encryptionkey", "credential",
	} {
		if strings.Contains(normalized, secret) {
			return true
		}
	}
	return false
}

// RedactDiagnosticValue returns a deep redacted copy of JSON-compatible data.
func RedactDiagnosticValue(value any) any {
	data, err := json.Marshal(value)
	if err != nil {
		return "[REDACTED]"
	}
	var decoded any
	if err := json.Unmarshal(data, &decoded); err != nil {
		return "[REDACTED]"
	}
	return RedactValue(decoded)
}
