//go:build darwin

package usage

import "strings"

// The Copilot CLI stores one token under copilot-cli per <host>:<login>.
// Reuse the bounded, timeout-protected Claude security process boundary.
func readCopilotKeychain(account string) string {
	blob, err := runSecurity("find-generic-password", "-w", "-s", "copilot-cli", "-a", account)
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(blob))
}
