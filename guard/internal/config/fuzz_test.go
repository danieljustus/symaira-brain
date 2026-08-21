package config

import (
	"os"
	"path/filepath"
	"testing"
)

// FuzzLoadFrom tests that arbitrary TOML content doesn't panic on load.
func FuzzLoadFrom(f *testing.F) {
	seeds := []string{
		"",
		"[defaults]\nshell = \"ask\"\n",
		"[[rules]]\nmatch.server = \"test\"\ndecision = \"allow\"\n",
		"invalid toml {{{",
		"\x00\x00\x00",
	}
	for _, s := range seeds {
		f.Add(s)
	}

	f.Fuzz(func(t *testing.T, tomlContent string) {
		dir := t.TempDir()
		path := filepath.Join(dir, "config.toml")
		if err := os.WriteFile(path, []byte(tomlContent), 0600); err != nil {
			t.Skip("write error:", err)
		}

		// Should never panic, only return an error for invalid content.
		_, _ = LoadFrom(path)
	})
}
