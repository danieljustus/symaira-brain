//go:build darwin

package usage

import (
	"reflect"
	"testing"
)

func TestCopilotKeychainSelectedAccountOnly(t *testing.T) {
	calls := fakeSecurity(t, map[string]string{"find-generic-password": "fixture-keychain\n"})
	if got := readCopilotKeychain("https://github.com:second"); got != "fixture-keychain" {
		t.Error("token mismatch")
	}
	want := [][]string{{"find-generic-password", "-w", "-s", "copilot-cli", "-a", "https://github.com:second"}}
	if !reflect.DeepEqual(*calls, want) {
		t.Errorf("args = %v", *calls)
	}
	if readCopilotKeychain("https://other.example:second") != "" {
		t.Error("untrusted host accepted")
	}
	if len(*calls) != 1 {
		t.Error("invalid account invoked security")
	}
}

func TestCopilotKeychainDenied(t *testing.T) {
	fakeSecurity(t, nil)
	if readCopilotKeychain("https://github.com:first") != "" {
		t.Error("failed read returned token")
	}
}
