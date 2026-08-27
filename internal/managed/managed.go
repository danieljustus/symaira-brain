package managed

import (
	"context"
	"fmt"
	"log/slog"
	"runtime"
)

// coreInstaller is the subset of *Installer that Setup/Fix depend on.
// Tests swap newInstaller to exercise the orchestration logic without
// touching the network.
type coreInstaller interface {
	Install(ctx context.Context, core *Core) error
}

// newInstaller creates the coreInstaller Setup/Fix use. Overridable in tests.
var newInstaller = func(binDir string) coreInstaller { return NewInstaller(binDir) }

// Setup downloads and installs all pinned core versions into binDir.
// It reports progress via the provided logger and returns an error if
// any core fails to install.
func Setup(ctx context.Context, binDir string, logger *slog.Logger) error {
	if logger == nil {
		logger = slog.Default()
	}

	manifest, err := LoadManifest()
	if err != nil {
		return err
	}

	inst := newInstaller(binDir)
	var failed, attempted int

	for name, core := range manifest.Cores {
		if !core.SupportsPlatform(runtime.GOOS) {
			logger.Info("skipping (platform)", "binary", name, "goos", runtime.GOOS)
			continue
		}
		attempted++

		logger.Info("installing", "binary", name, "version", core.Version, "repo", core.Repo)

		if err := inst.Install(ctx, &core); err != nil {
			logger.Error("failed to install", "binary", name, "error", err)
			failed++
			continue
		}

		logger.Info("installed", "binary", name, "version", core.Version)
	}

	if failed > 0 {
		// attempted, not len(manifest.Cores): a platform-restricted core
		// (e.g. macOS-only symcockpit) is skipped elsewhere, so the
		// denominator must count only the cores this run actually tried.
		return fmt.Errorf("managed: %d/%d cores failed to install", failed, attempted)
	}
	return nil
}

// Fix repairs any missing or version-mismatched managed binaries.
// It checks each core in the manifest: if the binary is missing or at
// a different version, it re-installs it. Already-correct binaries are
// skipped. Returns an error if any repair fails.
func Fix(ctx context.Context, binDir string, logger *slog.Logger) error {
	if logger == nil {
		logger = slog.Default()
	}

	manifest, err := LoadManifest()
	if err != nil {
		return err
	}

	inst := newInstaller(binDir)
	var repaired, skipped, failed, attempted int

	for name, core := range manifest.Cores {
		if !core.SupportsPlatform(runtime.GOOS) {
			logger.Info("skipping (platform)", "binary", name, "goos", runtime.GOOS)
			continue
		}
		attempted++

		existing, err := InstalledVersion(ctx, binDir, core.BinaryName)
		if err != nil {
			logger.Warn("cannot probe", "binary", name, "error", err)
		}

		// Compare versions with the "v" prefix normalized away: binaries
		// report bare semver ("0.15.3") while the manifest pins the tag
		// ("v0.15.3") — both must count as "already correct".
		if normalizeVersion(existing) == normalizeVersion(core.Version) {
			logger.Info("already correct", "binary", name, "version", existing)
			skipped++
			continue
		}

		if existing != "" {
			logger.Info("version mismatch, repairing", "binary", name,
				"installed", existing, "wanted", core.Version)
		} else {
			logger.Info("binary missing, installing", "binary", name, "version", core.Version)
		}

		if err := inst.Install(ctx, &core); err != nil {
			logger.Error("repair failed", "binary", name, "error", err)
			failed++
			continue
		}

		logger.Info("repaired", "binary", name, "version", core.Version)
		repaired++
	}

	logger.Info("doctor --fix complete",
		"repaired", repaired, "skipped", skipped, "failed", failed)

	if failed > 0 {
		return fmt.Errorf("managed: %d/%d cores failed to repair", failed, attempted)
	}
	return nil
}

// Status returns the installed version of each core binary, or empty
// string if not installed. This is used by `doctor` to report per-core
// version and origin.
func Status(ctx context.Context, binDir string) (map[string]string, error) {
	manifest, err := LoadManifest()
	if err != nil {
		return nil, err
	}

	versions := make(map[string]string, len(manifest.Cores))
	for name, core := range manifest.Cores {
		if !core.SupportsPlatform(runtime.GOOS) {
			continue
		}
		v, _ := InstalledVersion(ctx, binDir, core.BinaryName)
		versions[name] = v
	}
	return versions, nil
}
