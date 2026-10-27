package usage

import (
	"encoding/json"
	"errors"
	"net/http"
	"os"
)

func newCopilotProvider(client *http.Client, readKeychain func(string) string) *CopilotProvider {
	if client == nil {
		client = newProviderHTTPClient()
	}
	var cliErr error
	keychainSource := false
	token, source, err := resolveFileCredential("COPILOT_ACCESS_TOKEN", func() string {
		path := copilotCLIPath()
		if _, err := os.Lstat(path); os.IsNotExist(err) {
			return readCopilotToken(copilotConfigDir())
		}
		data, err := readCredentialFile(path)
		if err == nil {
			if !copilotCLIHasLogin(data) {
				return readCopilotToken(copilotConfigDir())
			}
			if account := copilotCLIAccount(data); account != "" {
				if token := copilotCLIFileToken(data, account); token != "" {
					return token
				}
				if token := readKeychain(account); token != "" {
					keychainSource = true
					return token
				}
			}
		}
		cliErr = errors.New(copilotCLILoginError)
		return ""
	})
	if err == nil && token == "" {
		err = cliErr
	}
	if keychainSource {
		source = "keychain"
	}
	return &CopilotProvider{accessToken: token, credSource: source, credErr: err, client: client}
}

func copilotCLIHasLogin(data []byte) bool {
	var config struct {
		Users []json.RawMessage `json:"loggedInUsers"`
		Last  json.RawMessage   `json:"lastLoggedInUser"`
	}
	if json.Unmarshal(stripCopilotJSONC(data), &config) != nil {
		return true
	}
	return len(config.Users) > 0 || len(config.Last) > 0 && string(config.Last) != "null"
}
