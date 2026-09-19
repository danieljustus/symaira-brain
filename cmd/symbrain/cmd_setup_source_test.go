package main

import (
	"bytes"
	"context"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/config"
	"github.com/danieljustus/symaira-brain/internal/managed"
)

// fakeSourceBuild writes a shell script that answers `version --json`
// with the given version, emulating a freshly built module binary.
func fakeSourceBuild(t *testing.T, version string) func(ctx context.Context, root string, spec sourceModuleSpec, destDir string) (string, string, error) {
	t.Helper()
	if runtime.GOOS == "windows" {
		t.Skip("fake shell binary not supported on windows")
	}
	return func(ctx context.Context, root string, spec sourceModuleSpec, destDir string) (string, string, error) {
		path := filepath.Join(destDir, spec.BinaryName)
		script := "#!/bin/sh\necho '{\"version\":\"" + version + "\"}'\n"
		if err := os.WriteFile(path, []byte(script), 0o755); err != nil {
			return "", "", err
		}
		return path, "fake-builder 1.0", nil
	}
}

func withFakeSourceBuild(t *testing.T, fn func(ctx context.Context, root string, spec sourceModuleSpec, destDir string) (string, string, error)) {
	t.Helper()
	orig := runSourceBuild
	runSourceBuild = fn
	t.Cleanup(func() { runSourceBuild = orig })
	origCommit := receiverCommit
	receiverCommit = func(root string) (string, error) { return "0123456789abcdef0123456789abcdef01234567", nil }
	t.Cleanup(func() { receiverCommit = origCommit })
}

func TestSelectSourceModules_ExplicitList(t *testing.T) {
	cfg := &config.Config{}
	specs, err := selectSourceModules("browse,scope", cfg)
	if err != nil {
		t.Fatalf("selectSourceModules: %v", err)
	}
	if len(specs) != 2 || specs[0].Module != "browse" || specs[1].Module != "scope" {
		t.Errorf("specs = %+v, want [browse scope] in declaration order", specs)
	}
}

func TestSelectSourceModules_ConfigDefault(t *testing.T) {
	cfg := &config.Config{Modules: config.ModulesConfig{Browse: true, Scope: true}}
	specs, err := selectSourceModules("", cfg)
	if err != nil {
		t.Fatalf("selectSourceModules: %v", err)
	}
	if len(specs) != 2 {
		t.Errorf("specs = %+v, want browse+scope from config", specs)
	}
}

func TestSelectSourceModules_EmptyIsError(t *testing.T) {
	if _, err := selectSourceModules("", &config.Config{}); err == nil {
		t.Fatal("empty selection: got nil, want error")
	}
}

func TestSelectSourceModules_UnknownIsError(t *testing.T) {
	if _, err := selectSourceModules("browse,nope", &config.Config{}); err == nil || !strings.Contains(err.Error(), "nope") {
		t.Fatalf("unknown module: got %v, want error naming the module", err)
	}
}

func TestPrepareSourceBuildLayout_UsesExternalStorage(t *testing.T) {
	volume := t.TempDir()
	originalVolume, originalGOOS, originalCI, originalMounted := sourceExternalVolume, sourceBuildGOOS, sourceBuildCI, sourceBuildVolumeMounted
	sourceExternalVolume = volume
	sourceBuildGOOS = "darwin"
	sourceBuildCI = func() bool { return false }
	sourceBuildVolumeMounted = func(string) bool { return true }
	t.Cleanup(func() {
		sourceExternalVolume, sourceBuildGOOS, sourceBuildCI, sourceBuildVolumeMounted = originalVolume, originalGOOS, originalCI, originalMounted
	})
	t.Setenv("SYMAIRA_EXTERNAL_BASE", filepath.Join(volume, "builds"))
	t.Setenv("SYMAIRA_EXTERNAL_RUNTIME_ROOT", filepath.Join(volume, "runtime"))

	layout, err := prepareSourceBuildLayout()
	if err != nil {
		t.Fatalf("prepareSourceBuildLayout: %v", err)
	}
	for _, path := range []string{layout.temp, layout.goCache, layout.goModCache, layout.cargoTarget, layout.swiftCache, layout.runtimeRoot} {
		if !strings.HasPrefix(path, volume+string(filepath.Separator)) {
			t.Errorf("path %q is outside external volume %q", path, volume)
		}
		if info, statErr := os.Stat(path); statErr != nil || !info.IsDir() {
			t.Errorf("external path %q is not a directory: %v", path, statErr)
		}
	}
}

func TestPrepareSourceBuildLayout_ReportsUnmountedVolume(t *testing.T) {
	volume := t.TempDir()
	originalVolume, originalGOOS, originalCI, originalMounted := sourceExternalVolume, sourceBuildGOOS, sourceBuildCI, sourceBuildVolumeMounted
	sourceExternalVolume = volume
	sourceBuildGOOS = "darwin"
	sourceBuildCI = func() bool { return false }
	sourceBuildVolumeMounted = func(string) bool { return false }
	t.Cleanup(func() {
		sourceExternalVolume, sourceBuildGOOS, sourceBuildCI, sourceBuildVolumeMounted = originalVolume, originalGOOS, originalCI, originalMounted
	})

	if _, err := prepareSourceBuildLayout(); err == nil || !strings.Contains(err.Error(), "not mounted") {
		t.Fatalf("prepareSourceBuildLayout error = %v, want explicit unmounted-volume error", err)
	}
}

func TestPrepareSourceBuildLayout_ReportsUnavailableVolume(t *testing.T) {
	originalVolume, originalGOOS, originalCI := sourceExternalVolume, sourceBuildGOOS, sourceBuildCI
	sourceExternalVolume = filepath.Join(t.TempDir(), "missing-volume")
	sourceBuildGOOS = "darwin"
	sourceBuildCI = func() bool { return false }
	t.Cleanup(func() {
		sourceExternalVolume, sourceBuildGOOS, sourceBuildCI = originalVolume, originalGOOS, originalCI
	})

	if _, err := prepareSourceBuildLayout(); err == nil || !strings.Contains(err.Error(), "required external build volume") {
		t.Fatalf("prepareSourceBuildLayout error = %v, want explicit unavailable-volume error", err)
	}
}

func TestPrepareSourceBuildLayout_SkipsExternalStorageInCI(t *testing.T) {
	originalVolume, originalGOOS, originalCI := sourceExternalVolume, sourceBuildGOOS, sourceBuildCI
	sourceExternalVolume = filepath.Join(t.TempDir(), "missing-volume")
	sourceBuildGOOS = "darwin"
	sourceBuildCI = func() bool { return true }
	t.Cleanup(func() {
		sourceExternalVolume, sourceBuildGOOS, sourceBuildCI = originalVolume, originalGOOS, originalCI
	})

	layout, err := prepareSourceBuildLayout()
	if err != nil {
		t.Fatalf("prepareSourceBuildLayout in CI: %v", err)
	}
	if layout.base != "" {
		t.Fatalf("CI layout base = %q, want empty", layout.base)
	}
}

func TestSetupFromSource_InstallsWithProvenance(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", filepath.Join(home, ".config"))

	// Config with browse module enabled.
	cfgDir := filepath.Join(home, ".config", "symbrain")
	if err := os.MkdirAll(cfgDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(cfgDir, "config.toml"), []byte("[modules]\nbrowse = true\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	withFakeSourceBuild(t, fakeSourceBuild(t, "0.8.0"))

	binDir := filepath.Join(home, ".symaira", "bin")
	var stdout, stderr bytes.Buffer
	code := runSetupFromSource(context.Background(), &stdout, &stderr, binDir, t.TempDir(), "", false)
	if code != 0 {
		t.Fatalf("runSetupFromSource exit = %d, stderr = %s", code, stderr.String())
	}
	if !strings.Contains(stdout.String(), "symbrowse") || !strings.Contains(stdout.String(), "0.8.0") {
		t.Errorf("stdout = %q, want symbrowse 0.8.0 installed line", stdout.String())
	}

	prov, err := managed.ReadProvenance(binDir, "symbrowse")
	if err != nil {
		t.Fatalf("ReadProvenance: %v", err)
	}
	if prov == nil || prov.Source != managed.SourceBrain {
		t.Fatalf("provenance = %+v, want brain-source", prov)
	}
	if prov.ReceiverCommit != "0123456789abcdef0123456789abcdef01234567" {
		t.Errorf("receiver commit = %q", prov.ReceiverCommit)
	}
	if prov.ModuleDir != "browse" || prov.Builder != "fake-builder 1.0" {
		t.Errorf("provenance = %+v", prov)
	}
	// Only the enabled module must be installed.
	if _, err := os.Stat(filepath.Join(binDir, "symoperate")); !os.IsNotExist(err) {
		t.Error("symoperate installed although operate module not selected")
	}
}

func TestSetupFromSource_UnknownModuleFailsBeforeBuild(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", filepath.Join(home, ".config"))

	builds := 0
	withFakeSourceBuild(t, func(ctx context.Context, root string, spec sourceModuleSpec, destDir string) (string, string, error) {
		builds++
		return fakeSourceBuild(t, "0.0.0")(ctx, root, spec, destDir)
	})

	var stdout, stderr bytes.Buffer
	code := runSetupFromSource(context.Background(), &stdout, &stderr, filepath.Join(home, ".symaira", "bin"), t.TempDir(), "nope", false)
	if code == 0 {
		t.Fatal("unknown module: exit 0, want non-zero")
	}
	if builds != 0 {
		t.Errorf("builder ran %d times for an unknown module, want 0 (fail before building)", builds)
	}
}

func TestSetupFromSource_BuildFailureFailsModule(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", filepath.Join(home, ".config"))

	withFakeSourceBuild(t, func(ctx context.Context, root string, spec sourceModuleSpec, destDir string) (string, string, error) {
		return "", "", context.DeadlineExceeded
	})

	var stdout, stderr bytes.Buffer
	code := runSetupFromSource(context.Background(), &stdout, &stderr, filepath.Join(home, ".symaira", "bin"), t.TempDir(), "browse", false)
	if code == 0 {
		t.Fatal("build failure: exit 0, want non-zero")
	}
	if _, err := os.Stat(filepath.Join(home, ".symaira", "bin", "symbrowse")); !os.IsNotExist(err) {
		t.Error("binary installed despite build failure")
	}
}
