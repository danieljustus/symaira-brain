package managed

import (
	"context"
	"runtime"
	"testing"
)

// TestFix_SkipsBrainSourceMismatch: a managed binary installed from the
// in-repo module sources (brain-source provenance) whose version no longer
// matches the manifest pin must NOT be replaced by a release download
// unless forced. The source build is intentional replacement state.
func TestFix_SkipsBrainSourceMismatch(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("fake shell binary not supported on windows")
	}
	binDir := t.TempDir()
	// All platform-supported cores at their pinned version except one
	// mismatched brain-source build.
	mismatched := ""
	for name, core := range mustManifest(t).Cores {
		if !core.SupportsPlatform(runtime.GOOS) {
			continue
		}
		if mismatched == "" {
			mismatched = name
			fakeVersionBinary(t, binDir, core.BinaryName, "0.0.0-local")
			if err := WriteProvenance(binDir, &Provenance{
				Binary:         core.BinaryName,
				Source:         SourceBrain,
				Version:        "0.0.0-local",
				ReceiverCommit: "0123456789abcdef",
				ModuleDir:      "browse",
			}); err != nil {
				t.Fatal(err)
			}
			continue
		}
		fakeVersionBinary(t, binDir, core.BinaryName, core.Version)
	}
	if mismatched == "" {
		t.Skip("no platform-supported cores in manifest")
	}

	fake := &fakeInstaller{}
	withFakeInstaller(t, fake)

	if err := Fix(context.Background(), binDir, nil, nil); err != nil {
		t.Fatalf("Fix: %v", err)
	}
	for _, called := range fake.calls {
		if called == mustManifest(t).Cores[mismatched].BinaryName {
			t.Errorf("Fix reinstalled brain-source binary %q without --force-release", called)
		}
	}
}

// TestFix_ForceReleaseRepairsBrainSource: with ForceRelease the same
// mismatch is repaired from the pinned release.
func TestFix_ForceReleaseRepairsBrainSource(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("fake shell binary not supported on windows")
	}
	binDir := t.TempDir()
	mismatched := ""
	for name, core := range mustManifest(t).Cores {
		if !core.SupportsPlatform(runtime.GOOS) {
			continue
		}
		if mismatched == "" {
			mismatched = name
			fakeVersionBinary(t, binDir, core.BinaryName, "0.0.0-local")
			if err := WriteProvenance(binDir, &Provenance{
				Binary: core.BinaryName, Source: SourceBrain, Version: "0.0.0-local",
			}); err != nil {
				t.Fatal(err)
			}
			continue
		}
		fakeVersionBinary(t, binDir, core.BinaryName, core.Version)
	}
	if mismatched == "" {
		t.Skip("no platform-supported cores in manifest")
	}

	fake := &fakeInstaller{}
	withFakeInstaller(t, fake)

	if err := FixWithOptions(context.Background(), binDir, nil, nil, FixOptions{ForceRelease: true}); err != nil {
		t.Fatalf("FixWithOptions: %v", err)
	}
	want := mustManifest(t).Cores[mismatched].BinaryName
	found := false
	for _, called := range fake.calls {
		if called == want {
			found = true
		}
	}
	if !found {
		t.Errorf("FixWithOptions(ForceRelease) did not repair %q; calls = %v", want, fake.calls)
	}
}

// TestFix_MismatchWithoutProvenanceRepairs preserves the pre-provenance
// behavior: a version-mismatched binary with no sidecar is repaired.
func TestFix_MismatchWithoutProvenanceRepairs(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("fake shell binary not supported on windows")
	}
	binDir := t.TempDir()
	mismatched := ""
	for name, core := range mustManifest(t).Cores {
		if !core.SupportsPlatform(runtime.GOOS) {
			continue
		}
		if mismatched == "" {
			mismatched = name
			fakeVersionBinary(t, binDir, core.BinaryName, "0.0.0-old")
			continue
		}
		fakeVersionBinary(t, binDir, core.BinaryName, core.Version)
	}
	if mismatched == "" {
		t.Skip("no platform-supported cores in manifest")
	}

	fake := &fakeInstaller{}
	withFakeInstaller(t, fake)

	if err := Fix(context.Background(), binDir, nil, nil); err != nil {
		t.Fatalf("Fix: %v", err)
	}
	want := mustManifest(t).Cores[mismatched].BinaryName
	found := false
	for _, called := range fake.calls {
		if called == want {
			found = true
		}
	}
	if !found {
		t.Errorf("Fix did not repair unprovenanced mismatch %q; calls = %v", want, fake.calls)
	}
}
