package managed

import (
	"context"
	"runtime"
	"strings"
	"testing"
)

// fakeManifest swaps the embedded manifest JSON for platform-skip coverage
// tests, then restores it on cleanup.
func fakeManifest(t *testing.T, json []byte) {
	t.Helper()
	orig := defaultManifestJSON
	defaultManifestJSON = json
	t.Cleanup(func() { defaultManifestJSON = orig })
}

// platformRestrictedManifest pins one core for every platform and one
// (symcockpit) restricted to darwin. On a non-darwin host the second core
// must be skipped, so Setup/Fix attempt only the first.
const platformRestrictedManifest = `{
  "schema_version": 1,
  "cores": {
    "symvault": {"version":"v0.15.3","repo":"danieljustus/symaira-vault","binary_name":"symvault","asset_prefix":"symaira-vault"},
    "symcockpit": {"version":"0.4.0","repo":"danieljustus/symaira-cockpit","binary_name":"symcockpit","asset_prefix":"symcockpit","platforms":["darwin"]}
  }
}`

func TestSetup_PlatformSkipDoesNotCountAsAttempted(t *testing.T) {
	fake := &fakeInstaller{}
	withFakeInstaller(t, fake)
	fakeManifest(t, []byte(platformRestrictedManifest))

	if runtime.GOOS == "darwin" {
		t.Skip("platform-skip path is unreachable on darwin in this test layout")
	}

	if err := Setup(context.Background(), t.TempDir(), nil); err != nil {
		t.Fatalf("Setup with platform-skip: %v", err)
	}
	if len(fake.calls) != 1 {
		t.Errorf("Install called %d times, want 1 (symcockpit is darwin-only)", len(fake.calls))
	}
	if len(fake.calls) == 1 && fake.calls[0] != "symvault" {
		t.Errorf("Install called for %q, want symvault", fake.calls[0])
	}
}

func TestFix_PlatformSkipDoesNotCountAsAttempted(t *testing.T) {
	fake := &fakeInstaller{}
	withFakeInstaller(t, fake)
	fakeManifest(t, []byte(platformRestrictedManifest))

	if runtime.GOOS == "darwin" {
		t.Skip("platform-skip path is unreachable on darwin in this test layout")
	}

	if err := Fix(context.Background(), t.TempDir(), nil); err != nil {
		t.Fatalf("Fix with platform-skip: %v", err)
	}
	if len(fake.calls) != 0 {
		t.Errorf("Fix installed %d cores, want 0 (symvault already correct, symcockpit darwin-only)", len(fake.calls))
	}
}

func TestSetup_PlatformSkipExcludedFromFailureFraction(t *testing.T) {
	fake := &fakeInstaller{failFor: map[string]bool{"symvault": true}}
	withFakeInstaller(t, fake)
	fakeManifest(t, []byte(platformRestrictedManifest))

	if runtime.GOOS == "darwin" {
		t.Skip("platform-skip failure-fraction path is unreachable on darwin in this test layout")
	}

	err := Setup(context.Background(), t.TempDir(), nil)
	if err == nil {
		t.Fatal("Setup with one failure among attempted cores: got nil error, want error")
	}
	if !strings.Contains(err.Error(), "1/1") {
		t.Errorf("Setup error = %q, want failure fraction to exclude skipped platform (1/1)", err.Error())
	}
}

func TestFix_PlatformSkipExcludedFromFailureFraction(t *testing.T) {
	fake := &fakeInstaller{failFor: map[string]bool{"symvault": true}}
	withFakeInstaller(t, fake)
	fakeManifest(t, []byte(platformRestrictedManifest))

	if runtime.GOOS == "darwin" {
		t.Skip("platform-skip failure-fraction path is unreachable on darwin in this test layout")
	}

	err := Fix(context.Background(), t.TempDir(), nil)
	if err == nil {
		t.Fatal("Fix with one failure among attempted cores: got nil error, want error")
	}
	if !strings.Contains(err.Error(), "1/1") {
		t.Errorf("Fix error = %q, want failure fraction to exclude skipped platform (1/1)", err.Error())
	}
}

func TestSetup_AllSupportedCoresAttempted(t *testing.T) {
	fake := &fakeInstaller{}
	withFakeInstaller(t, fake)
	fakeManifest(t, []byte(`{
  "schema_version": 1,
  "cores": {
    "symvault": {"version":"v0.15.3","repo":"danieljustus/symaira-vault","binary_name":"symvault","asset_prefix":"symaira-vault"},
    "symcockpit": {"version":"0.4.0","repo":"danieljustus/symaira-cockpit","binary_name":"symcockpit","asset_prefix":"symcockpit","platforms":["`+runtime.GOOS+`"]}
  }
}`))

	if err := Setup(context.Background(), t.TempDir(), nil); err != nil {
		t.Fatalf("Setup: %v", err)
	}
	if len(fake.calls) != 2 {
		t.Errorf("Install called %d times, want 2 supported cores attempted", len(fake.calls))
	}
}

func TestSetup_SingleFailureReturnsCorrectFraction(t *testing.T) {
	fake := &fakeInstaller{failFor: map[string]bool{"symvault": true}}
	withFakeInstaller(t, fake)
	fakeManifest(t, []byte(`{
  "schema_version": 1,
  "cores": {
    "symvault": {"version":"v0.15.3","repo":"danieljustus/symaira-vault","binary_name":"symvault","asset_prefix":"symaira-vault"}
  }
}`))

	err := Setup(context.Background(), t.TempDir(), nil)
	if err == nil {
		t.Fatal("Setup with one failing core: got nil error, want error")
	}
	if !strings.Contains(err.Error(), "1/1") {
		t.Errorf("Setup error = %q, want it to mention 1/1 failed", err.Error())
	}
}

func TestFix_SingleFailureReturnsCorrectFraction(t *testing.T) {
	fake := &fakeInstaller{failFor: map[string]bool{"symvault": true}}
	withFakeInstaller(t, fake)
	fakeManifest(t, []byte(`{
  "schema_version": 1,
  "cores": {
    "symvault": {"version":"v0.15.3","repo":"danieljustus/symaira-vault","binary_name":"symvault","asset_prefix":"symaira-vault"}
  }
}`))

	err := Fix(context.Background(), t.TempDir(), nil)
	if err == nil {
		t.Fatal("Fix with one failing core: got nil error, want error")
	}
	if !strings.Contains(err.Error(), "1/1") {
		t.Errorf("Fix error = %q, want it to mention 1/1 failed", err.Error())
	}
}

func TestSetup_TwoCoresOneFailureReportsTwoAsDenominator(t *testing.T) {
	fake := &fakeInstaller{failFor: map[string]bool{"symvault": true}}
	withFakeInstaller(t, fake)
	fakeManifest(t, []byte(`{
  "schema_version": 1,
  "cores": {
    "symvault": {"version":"v0.15.3","repo":"danieljustus/symaira-vault","binary_name":"symvault","asset_prefix":"symaira-vault"},
    "symfritz": {"version":"v0.15.3","repo":"danieljustus/symaira-fritz","binary_name":"symfritz","asset_prefix":"symaira-fritz"}
  }
}`))

	err := Setup(context.Background(), t.TempDir(), nil)
	if err == nil {
		t.Fatal("Setup with one failing core: got nil error, want error")
	}
	if !strings.Contains(err.Error(), "1/2") {
		t.Errorf("Setup error = %q, want it to contain 1/2", err.Error())
	}
}

func TestFix_TwoCoresOneFailureReportsTwoAsDenominator(t *testing.T) {
	fake := &fakeInstaller{failFor: map[string]bool{"symvault": true}}
	withFakeInstaller(t, fake)
	fakeManifest(t, []byte(`{
  "schema_version": 1,
  "cores": {
    "symvault": {"version":"v0.15.3","repo":"danieljustus/symaira-vault","binary_name":"symvault","asset_prefix":"symaira-vault"},
    "symfritz": {"version":"v0.15.3","repo":"danieljustus/symaira-fritz","binary_name":"symfritz","asset_prefix":"symaira-fritz"}
  }
}`))

	err := Fix(context.Background(), t.TempDir(), nil)
	if err == nil {
		t.Fatal("Fix with one failing core: got nil error, want error")
	}
	if !strings.Contains(err.Error(), "1/2") {
		t.Errorf("Fix error = %q, want it to contain 1/2", err.Error())
	}
}
