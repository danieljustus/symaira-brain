package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"runtime"
	"sort"

	"github.com/danieljustus/symaira-brain/internal/config"
	"github.com/danieljustus/symaira-brain/internal/managed"
	"github.com/danieljustus/symaira-brain/internal/xdg"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

type setupReport struct {
	BinDir  string       `json:"bin_dir"`
	Results []coreResult `json:"results"`
	Errors  []string     `json:"errors,omitempty"`
}

type coreResult struct {
	Name    string `json:"name"`
	Version string `json:"version"`
	Status  string `json:"status"` // "installed", "skipped", "error"
	Error   string `json:"error,omitempty"`
}

func cmdSetup(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	fs := flag.NewFlagSet("setup", flag.ContinueOnError)
	jsonOut := fs.Bool("json", false, "emit machine-readable JSON")
	fix := fs.Bool("fix", false, "repair missing or version-mismatched binaries (alias for doctor --fix)")
	forceRelease := fs.Bool("force-release", false, "with --fix: allow replacing a brain-source build with the pinned release download")
	allowUnsigned := fs.Bool("allow-unsigned", false, "install even if cosign or a core's signature is unavailable (prints a warning; skips publisher verification for that core)")
	fromSource := fs.String("from-source", "", "build optional module binaries from the in-repo sources at this repository root and install them into the managed directory (instead of downloading releases)")
	modulesFlag := fs.String("modules", "", "with --from-source: comma-separated module selection (browse,operate,scope); default: modules enabled in config")
	fs.SetOutput(stderr)
	if err := fs.Parse(normalizeFlags(args)); err != nil {
		return exitcodes.ExitNoInput
	}

	binDir, err := xdg.ManagedBinDir()
	if err != nil {
		fmt.Fprintf(stderr, "symbrain setup: %v\n", err)
		return exitcodes.ExitGeneric
	}

	if *fromSource != "" {
		if *fix || *allowUnsigned {
			fmt.Fprintf(stderr, "symbrain setup: --from-source cannot be combined with --fix or --allow-unsigned\n")
			return exitcodes.ExitNoInput
		}
		return runSetupFromSource(context.Background(), stdout, stderr, binDir, *fromSource, *modulesFlag, *jsonOut)
	}
	if *modulesFlag != "" {
		fmt.Fprintf(stderr, "symbrain setup: --modules requires --from-source\n")
		return exitcodes.ExitNoInput
	}

	if *fix {
		return runSetupFix(stdout, stderr, binDir, *jsonOut, *allowUnsigned, *forceRelease)
	}
	return runSetupInstall(stdout, stderr, binDir, *jsonOut, *allowUnsigned)
}

func runSetupInstall(stdout, stderr io.Writer, binDir string, jsonOut, allowUnsigned bool) exitcodes.ExitCode {
	ctx := context.Background()
	manifest, err := managed.LoadManifest()
	if err != nil {
		fmt.Fprintf(stderr, "symbrain setup: %v\n", err)
		return exitcodes.ExitGeneric
	}
	cfg, err := config.Load()
	if err != nil {
		fmt.Fprintf(stderr, "symbrain setup: %v\n", err)
		return exitcodes.ExitCodeFromError(err)
	}

	inst := managed.NewInstaller(binDir)
	inst.AllowUnsigned = allowUnsigned
	inst.Warn = stderr
	report := setupReport{BinDir: binDir}

	for _, name := range sortedCoreNames(manifest.ActiveCores(cfg.Modules.EnabledCores())) {
		core := manifest.ActiveCores(cfg.Modules.EnabledCores())[name]
		result := coreResult{Name: name, Version: core.Version}
		if !core.SupportsPlatform(runtime.GOOS) {
			result.Status = "skipped"
			if !jsonOut {
				fmt.Fprintf(stdout, "  -  %s %s (unsupported platform)\n", name, core.Version)
			}
			report.Results = append(report.Results, result)
			continue
		}

		if err := inst.Install(ctx, &core); err != nil {
			result.Status = "error"
			result.Error = err.Error()
			report.Errors = append(report.Errors, fmt.Sprintf("%s: %v", name, err))
			if !jsonOut {
				fmt.Fprintf(stderr, "  ✗  %s: %v\n", name, err)
			}
		} else {
			result.Status = "installed"
			if !jsonOut {
				fmt.Fprintf(stdout, "  ✓  %s %s\n", name, core.Version)
			}
		}
		report.Results = append(report.Results, result)
	}

	if jsonOut {
		if err := json.NewEncoder(stdout).Encode(report); err != nil {
			fmt.Fprintf(stderr, "symbrain setup: encode JSON: %v\n", err)
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

func runSetupFix(stdout, stderr io.Writer, binDir string, jsonOut, allowUnsigned, forceRelease bool) exitcodes.ExitCode {
	ctx := context.Background()
	manifest, err := managed.LoadManifest()
	if err != nil {
		fmt.Fprintf(stderr, "symbrain setup --fix: %v\n", err)
		return exitcodes.ExitGeneric
	}
	cfg, err := config.Load()
	if err != nil {
		fmt.Fprintf(stderr, "symbrain setup --fix: %v\n", err)
		return exitcodes.ExitCodeFromError(err)
	}

	inst := managed.NewInstaller(binDir)
	inst.AllowUnsigned = allowUnsigned
	inst.Warn = stderr
	report := setupReport{BinDir: binDir}
	var fixed, skipped int

	for _, name := range sortedCoreNames(manifest.ActiveCores(cfg.Modules.EnabledCores())) {
		core := manifest.ActiveCores(cfg.Modules.EnabledCores())[name]
		result := coreResult{Name: name, Version: core.Version}
		if !core.SupportsPlatform(runtime.GOOS) {
			result.Status = "skipped"
			skipped++
			if !jsonOut {
				fmt.Fprintf(stdout, "  -  %s %s (unsupported platform)\n", name, core.Version)
			}
			report.Results = append(report.Results, result)
			continue
		}

		existing, _ := managed.InstalledVersion(ctx, binDir, core.BinaryName)
		if managed.VersionsMatch(existing, core.Version) {
			result.Status = "skipped"
			skipped++
			if !jsonOut {
				fmt.Fprintf(stdout, "  ✓  %s %s (already installed)\n", name, existing)
			}
		} else if !forceRelease && isBrainSourceInstall(binDir, core.BinaryName) {
			// A binary built from the in-repo module sources is
			// intentional replacement state; release repair must not
			// silently overwrite it (see managed.FixWithOptions).
			result.Status = "skipped"
			skipped++
			if !jsonOut {
				fmt.Fprintf(stdout, "  -  %s %s (brain-source build; use --force-release to replace)\n", name, existing)
			}
		} else {
			if err := inst.Install(ctx, &core); err != nil {
				result.Status = "error"
				result.Error = err.Error()
				report.Errors = append(report.Errors, fmt.Sprintf("%s: %v", name, err))
				if !jsonOut {
					fmt.Fprintf(stderr, "  ✗  %s: %v\n", name, err)
				}
			} else {
				result.Status = "installed"
				fixed++
				if !jsonOut {
					fmt.Fprintf(stdout, "  ✓  %s %s (repaired)\n", name, core.Version)
				}
			}
		}
		report.Results = append(report.Results, result)
	}

	if jsonOut {
		if err := json.NewEncoder(stdout).Encode(report); err != nil {
			fmt.Fprintf(stderr, "symbrain setup --fix: encode JSON: %v\n", err)
			return exitcodes.ExitGeneric
		}
	} else {
		fmt.Fprintf(stdout, "\n%d fixed, %d already correct\n", fixed, skipped)
	}

	if len(report.Errors) > 0 {
		return exitcodes.ExitGeneric
	}
	return exitcodes.ExitOK
}

func sortedCoreNames(cores map[string]managed.Core) []string {
	names := make([]string, 0, len(cores))
	for name := range cores {
		names = append(names, name)
	}
	sort.Strings(names)
	return names
}
