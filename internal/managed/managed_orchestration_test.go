package managed

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"testing"
)

// fakeInstaller lets Setup/Fix orchestration be tested without touching the
// network. failFor names cores whose Install call should return an error.
type fakeInstaller struct {
	mu      sync.Mutex
	failFor map[string]bool
	calls   []string
}

func (f *fakeInstaller) Install(ctx context.Context, core *Core) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.calls = append(f.calls, core.BinaryName)
	if f.failFor[core.BinaryName] {
		return fmt.Errorf("fake install failure for %s", core.BinaryName)
	}
	return nil
}

func withFakeInstaller(t *testing.T, fake *fakeInstaller) {
	t.Helper()
	orig := newInstaller
	newInstaller = func(string) coreInstaller { return fake }
	t.Cleanup(func() { newInstaller = orig })
}

func TestSetup_AllSucceed(t *testing.T) {
	fake := &fakeInstaller{}
	withFakeInstaller(t, fake)

	if err := Setup(context.Background(), t.TempDir(), nil); err != nil {
		t.Fatalf("Setup: %v", err)
	}
	if len(fake.calls) != 1 {
		t.Errorf("Install called %d times, want 1 (one per manifest core)", len(fake.calls))
	}
}

func TestSetup_PartialFailureReturnsError(t *testing.T) {
	fake := &fakeInstaller{failFor: map[string]bool{"symvault": true}}
	withFakeInstaller(t, fake)

	err := Setup(context.Background(), t.TempDir(), nil)
	if err == nil {
		t.Fatal("Setup with one failing core: got nil error, want error")
	}
	if !strings.Contains(err.Error(), "1/1") {
		t.Errorf("Setup error = %q, want it to mention 1/1 failed", err.Error())
	}
}

// fakeVersionBinary writes an executable at binDir/name that prints
// {"version":"<version>"} for `<name> version --json`, so InstalledVersion
// can be exercised without a real managed binary.
func fakeVersionBinary(t *testing.T, binDir, name, version string) {
	t.Helper()
	if runtime.GOOS == "windows" {
		t.Skip("fake shell binary not supported on windows")
	}
	path := filepath.Join(binDir, name)
	script := fmt.Sprintf("#!/bin/sh\necho '{\"version\":\"%s\"}'\n", version)
	if err := os.WriteFile(path, []byte(script), 0o755); err != nil {
		t.Fatalf("write fake binary: %v", err)
	}
}

func TestFix_SkipsAlreadyCorrect(t *testing.T) {
	binDir := t.TempDir()
	// symvault is already at its pinned version (v0.15.3 per manifest.json) -> skip.
	fakeVersionBinary(t, binDir, "symvault", "v0.15.3")

	fake := &fakeInstaller{}
	withFakeInstaller(t, fake)

	if err := Fix(context.Background(), binDir, nil); err != nil {
		t.Fatalf("Fix: %v", err)
	}

	if len(fake.calls) != 0 {
		t.Errorf("Fix repaired %d cores, want 0", len(fake.calls))
	}
}

func TestFix_PartialFailureReturnsError(t *testing.T) {
	fake := &fakeInstaller{failFor: map[string]bool{"symvault": true}}
	withFakeInstaller(t, fake)

	err := Fix(context.Background(), t.TempDir(), nil)
	if err == nil {
		t.Fatal("Fix with one failing core: got nil error, want error")
	}
	if !strings.Contains(err.Error(), "failed to repair") {
		t.Errorf("Fix error = %q, want it to mention repair failure", err.Error())
	}
}

func TestStatus_ReportsInstalledVersion(t *testing.T) {
	binDir := t.TempDir()
	fakeVersionBinary(t, binDir, "symvault", "v0.15.3")

	versions, err := Status(context.Background(), binDir)
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	if versions["symvault"] != "v0.15.3" {
		t.Errorf("Status()[symvault] = %q, want v0.15.3", versions["symvault"])
	}
	if len(versions) != 1 {
		t.Errorf("Status returned %d entries, want 1 (one per manifest core)", len(versions))
	}
}

func TestDownloadURL(t *testing.T) {
	got := downloadURL("https://github.com", "danieljustus/symaira-vault", "symaira-vault_v0.15.3_darwin_arm64.tar.gz")
	want := "https://github.com/danieljustus/symaira-vault/releases/latest/download/symaira-vault_v0.15.3_darwin_arm64.tar.gz"
	if got != want {
		t.Errorf("downloadURL() = %q, want %q", got, want)
	}
}
