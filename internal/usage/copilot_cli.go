package usage

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
)

const copilotCLILoginError = "Copilot CLI login detected but selected account token is unavailable — unlock the macOS Keychain or sign in again with the Copilot CLI"

type copilotCLIUser struct {
	Host  string `json:"host"`
	Login string `json:"login"`
}

func validCopilotAccount(account string) bool {
	login, ok := strings.CutPrefix(account, "https://github.com:")
	if !ok || len(login) == 0 || len(login) > 39 {
		return false
	}
	for _, c := range login {
		if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '-') {
			return false
		}
	}
	return true
}

// Select exactly one account. Never fall back to a different logged-in user.
func copilotCLIAccount(data []byte) string {
	var config struct {
		Users []copilotCLIUser `json:"loggedInUsers"`
		Last  *copilotCLIUser  `json:"lastLoggedInUser"`
	}
	clean := stripCopilotJSONC(data)
	if json.Unmarshal(clean, &config) != nil {
		return ""
	}
	var selected copilotCLIUser
	if config.Last != nil {
		found := false
		for _, user := range config.Users {
			if user == *config.Last {
				selected = user
				found = true
				break
			}
		}
		if !found {
			return ""
		}
	} else if len(config.Users) == 1 {
		selected = config.Users[0]
	} else {
		return ""
	}
	account := selected.Host + ":" + selected.Login
	if !validCopilotAccount(account) {
		return ""
	}
	return account
}

func copilotCLIFileToken(data []byte, account string) string {
	var config struct {
		Tokens     map[string]string `json:"copilotTokens"`
		AuthTokens map[string]struct {
			Token string `json:"token"`
		} `json:"authTokens"`
	}
	if json.Unmarshal(stripCopilotJSONC(data), &config) != nil {
		return ""
	}
	if token := config.AuthTokens[account+":github"].Token; token != "" {
		return token
	}
	return config.Tokens[account]
}

func copilotCLIPath() string {
	home, _ := os.UserHomeDir()
	return filepath.Join(home, ".copilot", "config.json")
}

// Strip JSONC comments and trailing commas without touching URL/string bytes.
// Unterminated comments remain invalid JSON, rather than accepting a prefix.
func stripCopilotJSONC(data []byte) []byte {
	out := append([]byte(nil), bytes.TrimPrefix(data, []byte{0xef, 0xbb, 0xbf})...)
	quoted, escaped := false, false
	for i := 0; i < len(out); i++ {
		c := out[i]
		if quoted {
			if escaped {
				escaped = false
			} else if c == '\\' {
				escaped = true
			} else if c == '"' {
				quoted = false
			}
			continue
		}
		if c == '"' {
			quoted = true
			continue
		}
		if c != '/' || i+1 >= len(out) {
			continue
		}
		if out[i+1] == '/' {
			for ; i < len(out) && out[i] != '\n'; i++ {
				out[i] = ' '
			}
		} else if out[i+1] == '*' {
			end := bytes.Index(out[i+2:], []byte("*/"))
			if end < 0 {
				return nil
			}
			end += i + 4
			for ; i < end; i++ {
				out[i] = ' '
			}
			i--
		}
	}
	quoted, escaped = false, false
	for i, c := range out {
		if quoted {
			if escaped {
				escaped = false
			} else if c == '\\' {
				escaped = true
			} else if c == '"' {
				quoted = false
			}
			continue
		}
		if c == '"' {
			quoted = true
			continue
		}
		if c != ',' {
			continue
		}
		j := i + 1
		for j < len(out) && (out[j] == ' ' || out[j] == '\t' || out[j] == '\r' || out[j] == '\n') {
			j++
		}
		if j < len(out) && (out[j] == '}' || out[j] == ']') {
			out[i] = ' '
		}
	}
	return out
}
