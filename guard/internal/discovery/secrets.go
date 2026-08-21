package discovery

import "strings"

// secretKeyMarkers are substrings that mark an environment variable name as
// secret-bearing, matched case-insensitively.
var secretKeyMarkers = []string{
	"API_KEY", "TOKEN", "SECRET", "PASSWORD", "PASSWD", "CREDENTIAL", "PRIVATE_KEY", "AUTH",
}

// secretValuePrefixes are common literal prefixes of API keys and tokens
// (e.g. "sk-" for OpenAI/Anthropic-style keys, "ghp_" for GitHub PATs).
var secretValuePrefixes = []string{
	"sk-", "sk_", "ghp_", "gho_", "AKIA", "xoxb-", "xoxp-",
}

// LooksLikeSecret reports whether an environment key/value pair is likely a
// plaintext secret stored directly in a client config. The value must be
// non-empty and not a variable reference ("$NAME" or "${NAME}"), and either
// the key name or the value prefix must match a common secret shape. This is
// a heuristic for diagnostics only — it complements the redaction policy
// (see [Server.RedactedEnv]) rather than replacing it.
func LooksLikeSecret(key, value string) bool {
	if value == "" || isEnvReference(value) {
		return false
	}
	upper := strings.ToUpper(key)
	for _, m := range secretKeyMarkers {
		if strings.Contains(upper, m) {
			return true
		}
	}
	for _, p := range secretValuePrefixes {
		if strings.HasPrefix(value, p) {
			return true
		}
	}
	return false
}

// isEnvReference reports whether v is a variable reference ("$NAME" or
// "${NAME}") rather than a literal value.
func isEnvReference(v string) bool {
	if !strings.HasPrefix(v, "$") {
		return false
	}
	rest := v[1:]
	if strings.HasPrefix(rest, "{") && strings.HasSuffix(rest, "}") {
		return true
	}
	if rest == "" {
		return false
	}
	for _, r := range rest {
		if r != '_' && (r < 'a' || r > 'z') && (r < 'A' || r > 'Z') && (r < '0' || r > '9') {
			return false
		}
	}
	return true
}

// PlaintextSecretKeys returns the env keys whose values look like plaintext
// secrets stored directly in the client config. Keys whose values are
// variable references (e.g. "${API_KEY}") are not flagged. The returned
// order follows EnvKeys.
func (s Server) PlaintextSecretKeys() []string {
	var keys []string
	for i, k := range s.EnvKeys {
		if i < len(s.EnvValues) && LooksLikeSecret(k, s.EnvValues[i]) {
			keys = append(keys, k)
		}
	}
	return keys
}
