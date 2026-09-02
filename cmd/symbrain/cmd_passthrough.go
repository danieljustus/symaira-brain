// cmd_passthrough — passthrough subcommands that exec the external vault core.
//
// "symbrain vault <args...>"   → exec symvault <args...>
//
// Pure exec semantics: argv, stdin/stdout/stderr, exit code and TTY
// pass through untouched. Flag parsing by symbrain is intentionally
// skipped — everything after the subcommand name is opaque.

package main

import (
	"fmt"
	"io"
	"os"
	"os/exec"

	"github.com/danieljustus/symaira-brain/internal/broker"
	"github.com/danieljustus/symaira-brain/internal/config"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// passthroughMap links subcommand names to their core binary names.
var passthroughMap = map[string]string{
	"vault": "symvault",
}

// cmdPassthrough resolves the named core binary and exec's it with the
// given args. It never returns on success — the process replaces itself.
func cmdPassthrough(subcmd string, args []string, stderr io.Writer) exitcodes.ExitCode {
	binaryName, ok := passthroughMap[subcmd]
	if !ok {
		fmt.Fprintf(stderr, "symbrain: unknown passthrough %q\n", subcmd)
		return exitcodes.ExitNoInput
	}

	// Resolve the external vault binary: config override → managed dir
	// (~/.symaira/bin) → PATH lookup, per broker.Discover.
	override := ""
	if cfg, err := config.Load(); err == nil {
		override = cfg.Servers.Vault.BinaryPath
	}

	binPath, err := broker.Discover(binaryName, override)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain %s: %v\nHint: install %s or run `symbrain setup`.\n", subcmd, err, binaryName)
		return exitcodes.ExitGeneric
	}

	// Build the exec command: binary + all remaining args.
	cmd := exec.Command(binPath, args...)
	cmd.Stdin = os.Stdin
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr

	// Run and propagate the child's exit code.
	if err := cmd.Run(); err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			return exitcodes.ExitCode(exitErr.ExitCode())
		}
		fmt.Fprintf(stderr, "symbrain %s: %v\n", subcmd, err)
		return exitcodes.ExitGeneric
	}
	return exitcodes.ExitOK
}
