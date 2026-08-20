package vcs

import (
	"os"
	"strings"
	"testing"
)

// lastValue returns the effective value of key in an exec environment, which
// is the last assignment: exec applies later entries over earlier ones, and
// gitEnv appends its pins onto os.Environ().
func lastValue(env []string, key string) (string, bool) {
	value, found := "", false
	for _, entry := range env {
		name, rest, ok := strings.Cut(entry, "=")
		if ok && name == key {
			value, found = rest, true
		}
	}
	return value, found
}

// TestGitEnvPinsLocale guards the fix that made skill versioning
// language-independent. Without it a regression is invisible in CI — the
// runners are English, so the translated git output that broke import and
// history only appears on a developer machine with a non-English locale.
// This asserts the contract directly instead of relying on the environment
// the test happens to run in.
func TestGitEnvPinsLocale(t *testing.T) {
	// Hostile inherited values: the pins must win over them.
	t.Setenv("LC_ALL", "de_DE.UTF-8")
	t.Setenv("LANG", "de_DE.UTF-8")
	t.Setenv("LANGUAGE", "de:en")

	env := gitEnv()

	for key, want := range map[string]string{
		"LC_ALL":              "C",
		"LANG":                "C",
		"LANGUAGE":            "",
		"GIT_TERMINAL_PROMPT": "0",
	} {
		got, ok := lastValue(env, key)
		if !ok {
			t.Errorf("%s is not set in the git environment", key)
			continue
		}
		if got != want {
			t.Errorf("%s: got %q, want %q — an inherited value must not survive", key, got, want)
		}
	}
}

// TestGitEnvKeepsInheritedEnvironment: pinning the locale must not drop the
// rest of the environment, which git needs for PATH, HOME and credentials.
func TestGitEnvKeepsInheritedEnvironment(t *testing.T) {
	t.Setenv("SYMSKILLS_GITENV_PROBE", "kept")

	if got, ok := lastValue(gitEnv(), "SYMSKILLS_GITENV_PROBE"); !ok || got != "kept" {
		t.Errorf("inherited variable lost: got %q (present: %v)", got, ok)
	}
	if _, ok := lastValue(gitEnv(), "PATH"); !ok && os.Getenv("PATH") != "" {
		t.Error("PATH must survive into the git environment")
	}
}
