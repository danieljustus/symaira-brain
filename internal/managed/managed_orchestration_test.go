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

// supportedCoreNames returns the manifest core names whose platform list
// includes the current GOOS — the set Setup/Fix/Status actually act on.
func supportedCoreNames(t *testing.T) map[string]bool {
	t.Helper()
	m, err := LoadManifest()
	if err != nil {
		t.Fatalf("LoadManifest: %v", err)
	}
	out := make(map[string]bool, len(m.Cores))
	for name, core := range m.Cores {
		if core.SupportsPlatform(runtime.GOOS) {
			out[name] = true
		}
	}
	return out
}

func TestSetup_AllSucceed(t *testing.T) {
	fake := &fakeInstaller{}
	withFakeInstaller(t, fake)

	if err := Setup(context.Background(), t.TempDir(), nil); err != nil {
		t.Fatalf("Setup: %v", err)
	}
	if want := len(supportedCoreNames(t)); len(fake.calls) != want {
		t.Errorf("Install called %d times, want %d (one per platform-supported manifest core)", len(fake.calls), want)
	}
}

func TestSetup_PartialFailureReturnsError(t *testing.T) {
	fake := &fakeInstaller{failFor: map[string]bool{"symvault": true}}
	withFakeInstaller(t, fake)

	err := Setup(context.Background(), t.TempDir(), nil)
	if err == nil {
		t.Fatal("Setup with one failing core: got nil error, want error")
	}
	wantFrac := fmt.Sprintf("1/%d", len(supportedCoreNames(t)))
	if !strings.Contains(err.Error(), wantFrac) {
		t.Errorf("Setup error = %q, want it to mention %s failed", err.Error(), wantFrac)
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
	// Every platform-supported core is already at its pinned version
	// (per manifest.json) -> all skip.
	for _, core := range mustManifest(t).Cores {
		if core.SupportsPlatform(runtime.GOOS) {
			fakeVersionBinary(t, binDir, core.BinaryName, core.Version)
		}
	}

	fake := &fakeInstaller{}
	withFakeInstaller(t, fake)

	if err := Fix(context.Background(), binDir, nil); err != nil {
		t.Fatalf("Fix: %v", err)
	}

	if len(fake.calls) != 0 {
		t.Errorf("Fix repaired %d cores, want 0", len(fake.calls))
	}
}

func mustManifest(t *testing.T) *Manifest {
	t.Helper()
	m, err := LoadManifest()
	if err != nil {
		t.Fatalf("LoadManifest: %v", err)
	}
	return m
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
	// Install a fake binary per platform-supported core.
	supported := supportedCoreNames(t)
	for name, core := range mustManifest(t).Cores {
		if supported[name] {
			fakeVersionBinary(t, binDir, core.BinaryName, core.Version)
		}
	}

	versions, err := Status(context.Background(), binDir)
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	wantVault := mustManifest(t).Cores["symvault"].Version
	if versions["symvault"] != wantVault {
		t.Errorf("Status()[symvault] = %q, want %q", versions["symvault"], wantVault)
	}
	wantCockpit := mustManifest(t).Cores["symcockpit"].Version
	if supported["symcockpit"] && versions["symcockpit"] != wantCockpit {
		t.Errorf("Status()[symcockpit] = %q, want %q", versions["symcockpit"], wantCockpit)
	}
	if len(versions) != len(supported) {
		t.Errorf("Status returned %d entries, want %d (one per platform-supported core)", len(versions), len(supported))
	}
	// A platform-restricted core never appears on an unsupported GOOS.
	if !supported["symcockpit"] {
		if _, ok := versions["symcockpit"]; ok {
			t.Error("Status() includes symcockpit on a non-darwin platform, want omitted")
		}
	}
}

func TestDownloadURL(t *testing.T) {
	got := downloadURL("https://github.com", "danieljustus/symaira-vault", "v0.15.3", "symaira-vault_0.15.3_darwin_arm64.tar.gz")
	want := "https://github.com/danieljustus/symaira-vault/releases/download/v0.15.3/symaira-vault_0.15.3_darwin_arm64.tar.gz"
	if got != want {
		t.Errorf("downloadURL() = %q, want %q", got, want)
	}
}
