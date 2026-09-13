package main

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/config"
	"github.com/danieljustus/symaira-brain/internal/managed"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// sourceModuleSpec describes how one optional module binary is built from
// the in-repo receiving sources (PB-2026-09-09 §2 source intakes).
type sourceModuleSpec struct {
	// Module is the config key under [modules] ("browse", "operate",
	// "scope").
	Module string
	// BinaryName is the managed binary name installed into the managed
	// bin directory.
	BinaryName string
	// Dir is the module source directory relative to the repository root.
	Dir string
	// DarwinOnly marks modules whose toolchain (Swift) and runtime
	// contract only exist on macOS.
	DarwinOnly bool
}

// sourceModules is the full buildable set. Order is stable for
// deterministic reports.
var sourceModules = []sourceModuleSpec{
	{Module: "browse", BinaryName: "symbrowse", Dir: "browse"},
	{Module: "operate", BinaryName: "symoperate", Dir: "operate", DarwinOnly: true},
	{Module: "scope", BinaryName: "symscope", Dir: "scope", DarwinOnly: true},
}

// sourceResult is the per-module outcome of a --from-source install.
type sourceResult struct {
	Module         string `json:"module"`
	Binary         string `json:"binary"`
	Version        string `json:"version,omitempty"`
	Status         string `json:"status"` // "installed", "skipped", "error"
	Source         string `json:"source,omitempty"`
	ReceiverCommit string `json:"receiver_commit,omitempty"`
	BinarySHA256   string `json:"binary_sha256,omitempty"`
	Error          string `json:"error,omitempty"`
}

type setupSourceReport struct {
	BinDir  string         `json:"bin_dir"`
	Root    string         `json:"root"`
	Results []sourceResult `json:"results"`
	Errors  []string       `json:"errors,omitempty"`
}

// Overridable in tests.
var (
	// runSourceBuild builds one module binary from the receiving sources
	// into destDir and returns the binary path plus a builder identity
	// string (toolchain name+version) for provenance.
	runSourceBuild = buildModuleBinary
	// receiverCommit resolves the symaira-brain commit the sources are
	// built from. Provenance without a commit is worthless, so callers
	// treat an error here as fatal for the affected module.
	receiverCommit = func(root string) (string, error) {
		out, err := exec.Command("git", "-C", root, "rev-parse", "HEAD").Output()
		if err != nil {
			return "", fmt.Errorf("resolve receiver commit: %w", err)
		}
		return strings.TrimSpace(string(out)), nil
	}
)

// buildModuleBinary is the production builder: Go for browse/, SwiftPM
// for operate/ and scope/.
func buildModuleBinary(ctx context.Context, root string, spec sourceModuleSpec, destDir string) (string, string, error) {
	switch spec.Dir {
	case "browse":
		return buildBrowseBinary(ctx, root, destDir)
	default:
		return buildSwiftModuleBinary(ctx, root, spec, destDir)
	}
}

func buildBrowseBinary(ctx context.Context, root, destDir string) (string, string, error) {
	if _, err := exec.LookPath("go"); err != nil {
		return "", "", fmt.Errorf("go toolchain not found on PATH: %w", err)
	}
	out, err := exec.CommandContext(ctx, "go", "version").Output()
	if err != nil {
		return "", "", fmt.Errorf("go version: %w", err)
	}
	builder := strings.TrimSpace(string(out))

	target := filepath.Join(destDir, "symbrowse")
	cmd := exec.CommandContext(ctx, "go", "build", "-trimpath", "-o", target, "./cmd/symbrowse")
	cmd.Dir = filepath.Join(root, "browse")
	cmd.Env = append(os.Environ(), "CGO_ENABLED=0")
	if combined, err := cmd.CombinedOutput(); err != nil {
		return "", "", fmt.Errorf("go build ./cmd/symbrowse: %w\n%s", err, combined)
	}
	return target, builder, nil
}

func buildSwiftModuleBinary(ctx context.Context, root string, spec sourceModuleSpec, destDir string) (string, string, error) {
	if _, err := exec.LookPath("swift"); err != nil {
		return "", "", fmt.Errorf("swift toolchain not found on PATH: %w", err)
	}
	out, err := exec.CommandContext(ctx, "swift", "--version").Output()
	if err != nil {
		return "", "", fmt.Errorf("swift --version: %w", err)
	}
	builder := strings.SplitN(strings.TrimSpace(string(out)), "\n", 2)[0]

	pkgPath := filepath.Join(root, spec.Dir)
	if buildOut, err := exec.CommandContext(ctx, "swift", "build", "--package-path", pkgPath).CombinedOutput(); err != nil {
		return "", "", fmt.Errorf("swift build (%s): %w\n%s", spec.Dir, err, buildOut)
	}
	binOut, err := exec.CommandContext(ctx, "swift", "build", "--package-path", pkgPath, "--show-bin-path").Output()
	if err != nil {
		return "", "", fmt.Errorf("swift build --show-bin-path (%s): %w", spec.Dir, err)
	}
	built := filepath.Join(strings.TrimSpace(string(binOut)), spec.BinaryName)
	if _, err := os.Stat(built); err != nil {
		return "", "", fmt.Errorf("expected built binary %s: %w", built, err)
	}
	// Copy into destDir so later package rebuilds cannot swap the payload
	// out from under the managed install.
	target := filepath.Join(destDir, spec.BinaryName)
	data, err := os.ReadFile(built)
	if err != nil {
		return "", "", fmt.Errorf("read built binary %s: %w", built, err)
	}
	if err := os.WriteFile(target, data, 0o755); err != nil {
		return "", "", fmt.Errorf("stage built binary: %w", err)
	}
	return target, builder, nil
}

// selectSourceModules resolves which modules a --from-source run builds:
// the explicit --modules list when given, otherwise the modules enabled in
// the global config. An empty result is an error — silently building
// nothing would report success for work that never happened.
func selectSourceModules(modulesFlag string, cfg *config.Config) ([]sourceModuleSpec, error) {
	wanted := map[string]bool{}
	if modulesFlag != "" {
		for _, name := range strings.Split(modulesFlag, ",") {
			name = strings.TrimSpace(name)
			if name != "" {
				wanted[name] = true
			}
		}
	} else {
		if cfg.Modules.Browse {
			wanted["browse"] = true
		}
		if cfg.Modules.Operate {
			wanted["operate"] = true
		}
		if cfg.Modules.Scope {
			wanted["scope"] = true
		}
	}
	if len(wanted) == 0 {
		return nil, fmt.Errorf("no modules selected: enable modules in config ([modules] browse/operate/scope) or pass --modules browse,operate,scope")
	}
	var specs []sourceModuleSpec
	for _, spec := range sourceModules {
		if wanted[spec.Module] {
			specs = append(specs, spec)
			delete(wanted, spec.Module)
		}
	}
	for unknown := range wanted {
		return nil, fmt.Errorf("unknown module %q (known: browse, operate, scope)", unknown)
	}
	return specs, nil
}

func runSetupFromSource(ctx context.Context, stdout, stderr io.Writer, binDir, root, modulesFlag string, jsonOut bool) exitcodes.ExitCode {
	absRoot, err := filepath.Abs(root)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain setup --from-source: %v\n", err)
		return exitcodes.ExitNoInput
	}
	info, err := os.Stat(absRoot)
	if err != nil || !info.IsDir() {
		fmt.Fprintf(stderr, "symbrain setup --from-source: %s is not a directory\n", absRoot)
		return exitcodes.ExitNoInput
	}

	cfg, err := config.Load()
	if err != nil {
		fmt.Fprintf(stderr, "symbrain setup --from-source: load config: %v\n", err)
		return exitcodes.ExitCodeFromError(err)
	}
	specs, err := selectSourceModules(modulesFlag, cfg)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain setup --from-source: %v\n", err)
		return exitcodes.ExitNoInput
	}

	commit, err := receiverCommit(absRoot)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain setup --from-source: %v\n", err)
		return exitcodes.ExitGeneric
	}

	inst := managed.NewInstaller(binDir)
	report := setupSourceReport{BinDir: binDir, Root: absRoot}

	for _, spec := range specs {
		result := sourceResult{Module: spec.Module, Binary: spec.BinaryName}

		if spec.DarwinOnly && runtime.GOOS != "darwin" {
			result.Status = "skipped"
			result.Error = "unsupported platform (darwin only)"
			if !jsonOut {
				fmt.Fprintf(stdout, "  -  %s (unsupported platform)\n", spec.BinaryName)
			}
			report.Results = append(report.Results, result)
			continue
		}

		tmpDir, err := os.MkdirTemp("", "symbrain-source-build-*")
		if err != nil {
			result.Status = "error"
			result.Error = err.Error()
			report.Errors = append(report.Errors, fmt.Sprintf("%s: %v", spec.Module, err))
			report.Results = append(report.Results, result)
			continue
		}

		binaryPath, builder, buildErr := runSourceBuild(ctx, absRoot, spec, tmpDir)
		if buildErr != nil {
			os.RemoveAll(tmpDir)
			result.Status = "error"
			result.Error = buildErr.Error()
			report.Errors = append(report.Errors, fmt.Sprintf("%s: %v", spec.Module, buildErr))
			if !jsonOut {
				fmt.Fprintf(stderr, "  ✗  %s: %v\n", spec.BinaryName, buildErr)
			}
			report.Results = append(report.Results, result)
			continue
		}

		data, err := os.ReadFile(binaryPath)
		if err != nil {
			os.RemoveAll(tmpDir)
			result.Status = "error"
			result.Error = err.Error()
			report.Errors = append(report.Errors, fmt.Sprintf("%s: %v", spec.Module, err))
			report.Results = append(report.Results, result)
			continue
		}

		prov := &managed.Provenance{
			Source:         managed.SourceBrain,
			ReceiverCommit: commit,
			ModuleDir:      spec.Dir,
			Builder:        builder,
		}
		if err := inst.InstallLocal(spec.BinaryName, data, prov); err != nil {
			os.RemoveAll(tmpDir)
			result.Status = "error"
			result.Error = err.Error()
			report.Errors = append(report.Errors, fmt.Sprintf("%s: %v", spec.Module, err))
			report.Results = append(report.Results, result)
			continue
		}
		os.RemoveAll(tmpDir)

		// Version handshake with the installed artifact, not the build
		// tree: the recorded version must come from what is actually
		// installed in the managed directory.
		version, err := managed.InstalledVersion(ctx, binDir, spec.BinaryName)
		if err != nil {
			result.Status = "error"
			result.Error = fmt.Sprintf("installed but version probe failed: %v", err)
			report.Errors = append(report.Errors, fmt.Sprintf("%s: %v", spec.Module, err))
			report.Results = append(report.Results, result)
			continue
		}

		result.Status = "installed"
		result.Version = version
		result.Source = string(managed.SourceBrain)
		result.ReceiverCommit = commit
		installed, _ := managed.ReadProvenance(binDir, spec.BinaryName)
		if installed != nil {
			result.BinarySHA256 = installed.BinarySHA256
		}
		if !jsonOut {
			fmt.Fprintf(stdout, "  ✓  %s %s (brain-source, %s)\n", spec.BinaryName, version, shortCommit(commit))
		}
		report.Results = append(report.Results, result)
	}

	if jsonOut {
		if err := json.NewEncoder(stdout).Encode(report); err != nil {
			fmt.Fprintf(stderr, "symbrain setup --from-source: encode JSON: %v\n", err)
			return exitcodes.ExitGeneric
		}
	} else {
		fmt.Fprintf(stdout, "\nInstalled to %s\n", binDir)
	}

	if len(report.Errors) > 0 {
		return exitcodes.ExitGeneric
	}
	return exitcodes.ExitOK
}

func shortCommit(commit string) string {
	if len(commit) > 12 {
		return commit[:12]
	}
	return commit
}

// isBrainSourceInstall reports whether the managed binary was installed
// from the in-repo module sources (setup --from-source). An unreadable or
// absent sidecar answers false — the release repair path then applies.
func isBrainSourceInstall(binDir, binaryName string) bool {
	prov, err := managed.ReadProvenance(binDir, binaryName)
	return err == nil && prov != nil && prov.Source == managed.SourceBrain
}
