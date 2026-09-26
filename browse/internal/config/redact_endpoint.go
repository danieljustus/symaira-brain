package config

import (
	"net/url"
	"strings"
)

// redactEndpoint keeps safe connection details while removing credentials
// that config show must never echo from TOML or the environment.
func redactEndpoint(raw string) string {
	if raw == "" {
		return raw
	}
	parsed, err := url.Parse(raw)
	if err != nil || parsed.Scheme == "" || parsed.Host == "" {
		return "[REDACTED]"
	}
	changed := false
	if parsed.User != nil {
		parsed.User = nil
		changed = true
	}
	query := parsed.Query()
	for key, values := range query {
		if secretEndpointKey(key) {
			query[key] = []string{"[REDACTED]"}
			changed = true
		} else {
			query[key] = values
		}
	}
	if !changed {
		return raw
	}
	parsed.RawQuery = query.Encode()
	return parsed.String()
}

func secretEndpointKey(key string) bool {
	key = strings.NewReplacer("_", "", "-", "").Replace(strings.ToLower(key))
	for _, secret := range []string{
		"password", "passwd", "secret", "token", "authorization", "auth", "cookie",
		"credential", "apikey", "accesskey", "privatekey", "encryptionkey",
	} {
		if strings.Contains(key, secret) {
			return true
		}
	}
	return false
}
