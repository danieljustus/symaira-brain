//go:build !darwin

package usage

func readCopilotKeychain(string) string { return "" }
