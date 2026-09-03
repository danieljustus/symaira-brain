package main

import (
	"context"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"
)

func TestCheckVaultReachable_NotInstalled(t *testing.T) {
	// Hide symvault from PATH so LookPath fails deterministically
	// regardless of what is installed on this machine.
	t.Setenv("PATH", t.TempDir())

	got := checkVaultReachable(context.Background())
	if got.Status != linkUnknown {
		t.Errorf("Status = %q, want %q (symvault not on PATH is unknown, not a failure)", got.Status, linkUnknown)
	}
	if got.Remedy == "" {
		t.Error("expected a remedy pointing at `symbrain setup`")
	}
}

func TestCheckVaultReachable_NeverAttemptsToUnlock(t *testing.T) {
	// A fake symvault that would fail the test if invoked with "unlock".
	if runtime.GOOS == "windows" {
		t.Skip("fake shell binary not supported on windows")
	}
	dir := t.TempDir()
	script := `#!/bin/sh
for arg in "$@"; do
  if [ "$arg" = "unlock" ]; then
    echo "TEST FAILURE: checkVaultReachable must never call unlock" >&2
    exit 99
  fi
done
echo "Error: entry not found: probe: entry not found" >&2
exit 2
`
	if err := os.WriteFile(filepath.Join(dir, "symvault"), []byte(script), 0o755); err != nil {
		t.Fatalf("write fake symvault: %v", err)
	}
	t.Setenv("PATH", dir)

	got := checkVaultReachable(context.Background())
	if got.Status != linkPass {
		t.Errorf("Status = %q, want %q", got.Status, linkPass)
	}
}

func TestCheckVaultReachable_LockedIsPassNotFail(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("fake shell binary not supported on windows")
	}
	dir := t.TempDir()
	script := "#!/bin/sh\necho 'Error: vault is locked, run symvault unlock' >&2\nexit 1\n"
	if err := os.WriteFile(filepath.Join(dir, "symvault"), []byte(script), 0o755); err != nil {
		t.Fatalf("write fake symvault: %v", err)
	}
	t.Setenv("PATH", dir)

	got := checkVaultReachable(context.Background())
	if got.Status != linkPass {
		t.Errorf("Status = %q, want %q (locked is a valid, reportable state, not a failure)", got.Status, linkPass)
	}
	if !strings.Contains(got.Detail, "locked") {
		t.Errorf("Detail = %q, want it to mention the vault is locked", got.Detail)
	}
}

func TestCheckVaultReachable_UnexpectedErrorIsFail(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("fake shell binary not supported on windows")
	}
	dir := t.TempDir()
	script := "#!/bin/sh\necho 'Error: vault directory is corrupt' >&2\nexit 1\n"
	if err := os.WriteFile(filepath.Join(dir, "symvault"), []byte(script), 0o755); err != nil {
		t.Fatalf("write fake symvault: %v", err)
	}
	t.Setenv("PATH", dir)

	got := checkVaultReachable(context.Background())
	if got.Status != linkFail {
		t.Errorf("Status = %q, want %q", got.Status, linkFail)
	}
}

func TestCheckVaultReachable_TimesOut(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("fake shell binary not supported on windows")
	}
	dir := t.TempDir()
	// "exec sleep" replaces the shell's own process image instead of
	// forking a child, so killing this process (what CommandContext
	// does on ctx cancellation) actually stops it within the timeout —
	// a forked, unreplaced child would keep the stderr pipe open and
	// make Cmd.Wait() block until the child exits on its own.
	script := "#!/bin/sh\nexec /bin/sleep 30\n"
	if err := os.WriteFile(filepath.Join(dir, "symvault"), []byte(script), 0o755); err != nil {
		t.Fatalf("write fake symvault: %v", err)
	}
	t.Setenv("PATH", dir)

	ctx, cancel := context.WithTimeout(context.Background(), 200*time.Millisecond)
	defer cancel()

	got := checkVaultReachable(ctx)
	if got.Status != linkUnknown {
		t.Errorf("Status = %q, want %q", got.Status, linkUnknown)
	}
}

func TestResolveSecretReferenceCheck_NeverLeaksResolvedValue(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("fake shell binary not supported on windows")
	}
	// A fake symvault that resolves to a known secret value. The check's
	// entire report (Name, Status, Detail, Remedy) must never contain it.
	const secretValue = "s3cr3t-value-that-must-not-leak"
	dir := t.TempDir()
	script := "#!/bin/sh\necho '" + secretValue + "'\n"
	if err := os.WriteFile(filepath.Join(dir, "symvault"), []byte(script), 0o755); err != nil {
		t.Fatalf("write fake symvault: %v", err)
	}
	t.Setenv("PATH", dir)

	got := resolveSecretReferenceCheck(configuredSecretRef{
		source: "test", value: "symvault://some/path",
	})

	blob := got.Name + got.Detail + got.Remedy + string(got.Status)
	if strings.Contains(blob, secretValue) {
		t.Fatalf("resolveSecretReferenceCheck leaked the resolved secret value: %+v", got)
	}
	if got.Status != linkPass {
		t.Errorf("Status = %q, want %q", got.Status, linkPass)
	}
}

func TestCheckSecretReferences_NoneConfiguredIsUnknown(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", "")

	got := checkSecretReferences()
	if len(got) != 1 || got[0].Status != linkUnknown {
		t.Errorf("checkSecretReferences() with nothing configured = %+v, want one linkUnknown entry", got)
	}
}

func TestCheckProfilesRegistered_UnregisteredProfileFails(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)

	profilesDir := filepath.Join(home, ".config", "symbrain", "profiles")
	if err := os.MkdirAll(profilesDir, 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}
	if err := os.WriteFile(filepath.Join(profilesDir, "orphan.toml"), []byte("name = \"orphan\"\n"), 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	got := checkProfilesRegistered(nil)
	if len(got) != 1 {
		t.Fatalf("checkProfilesRegistered() = %d entries, want 1", len(got))
	}
	if got[0].Status != linkFail {
		t.Errorf("Status = %q, want %q (no harness registers this profile)", got[0].Status, linkFail)
	}
	if got[0].Remedy == "" {
		t.Error("expected a remedy naming `symbrain install`")
	}
}

func TestCheckProfilesRegistered_RegisteredProfilePasses(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)

	profilesDir := filepath.Join(home, ".config", "symbrain", "profiles")
	if err := os.MkdirAll(profilesDir, 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}
	if err := os.WriteFile(filepath.Join(profilesDir, "personal.toml"), []byte("name = \"personal\"\n"), 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	harnesses := []harnessCheck{
		{Name: "claude", Installed: true, Profile: "personal"},
	}

	got := checkProfilesRegistered(harnesses)
	if len(got) != 1 {
		t.Fatalf("checkProfilesRegistered() = %d entries, want 1", len(got))
	}
	if got[0].Status != linkPass {
		t.Errorf("Status = %q, want %q", got[0].Status, linkPass)
	}
}

func TestCheckProfilesRegistered_NoProfilesReturnsNoChecks(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)

	if got := checkProfilesRegistered(nil); len(got) != 0 {
		t.Errorf("checkProfilesRegistered() with no profiles = %v, want empty", got)
	}
}
