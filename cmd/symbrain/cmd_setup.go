package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"io"

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
	allowUnsigned := fs.Bool("allow-unsigned", false, "install even if cosign or a core's signature is unavailable (prints a warning; skips publisher verification for that core)")
	fs.SetOutput(stderr)
	if err := fs.Parse(normalizeFlags(args)); err != nil {
		return exitcodes.ExitNoInput
	}

	binDir, err := xdg.ManagedBinDir()
	if err != nil {
		fmt.Fprintf(stderr, "symbrain setup: %v\n", err)
		return exitcodes.ExitGeneric
	}

	if *fix {
		return runSetupFix(stdout, stderr, binDir, *jsonOut, *allowUnsigned)
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

	inst := managed.NewInstaller(binDir)
	inst.AllowUnsigned = allowUnsigned
	inst.Warn = stderr
	report := setupReport{BinDir: binDir}

	for name, core := range manifest.Cores {
		result := coreResult{Name: name, Version: core.Version}

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

func runSetupFix(stdout, stderr io.Writer, binDir string, jsonOut, allowUnsigned bool) exitcodes.ExitCode {
	ctx := context.Background()
	manifest, err := managed.LoadManifest()
	if err != nil {
		fmt.Fprintf(stderr, "symbrain setup --fix: %v\n", err)
		return exitcodes.ExitGeneric
	}

	inst := managed.NewInstaller(binDir)
	inst.AllowUnsigned = allowUnsigned
	inst.Warn = stderr
	report := setupReport{BinDir: binDir}
	var fixed, skipped int

	for name, core := range manifest.Cores {
		result := coreResult{Name: name, Version: core.Version}

		existing, _ := managed.InstalledVersion(ctx, binDir, core.BinaryName)
		if existing == core.Version {
			result.Status = "skipped"
			skipped++
			if !jsonOut {
				fmt.Fprintf(stdout, "  ✓  %s %s (already installed)\n", name, existing)
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
