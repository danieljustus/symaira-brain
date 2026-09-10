package main

import (
	"flag"
	"fmt"
	"io"

	"github.com/danieljustus/symaira-brain/internal/harness"
	"github.com/danieljustus/symaira-brain/internal/profile"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

func cmdProfileRemove(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	args = reorderFlagsFirst(args, map[string]bool{"project": true})

	fs := flag.NewFlagSet("profile remove", flag.ContinueOnError)
	force := fs.Bool("force", false, "skip the confirmation prompt")
	projectDir := fs.String("project", "", "project directory; check project-local harness configs for bindings")
	fs.SetOutput(stderr)
	if err := fs.Parse(normalizeFlags(args)); err != nil {
		return exitcodes.ExitNoInput
	}
	if fs.NArg() != 1 {
		fmt.Fprintln(stderr, "usage: symbrain profile remove <name> [--force] [--project <dir>]")
		return exitcodes.ExitNoInput
	}
	name := fs.Arg(0)

	if err := profile.ValidateName(name); err != nil {
		fmt.Fprintf(stderr, "symbrain profile remove: %v\n", err)
		return exitcodes.ExitNoInput
	}
	if !profile.Exists(name) {
		fmt.Fprintf(stderr, "symbrain profile remove: profile %q does not exist\n", name)
		return exitcodes.ExitNoInput
	}

	scan := harness.ProfileBindings(name, *projectDir)
	if !*force && (len(scan.Errors) > 0 || len(scan.Bindings) > 0) {
		if len(scan.Errors) > 0 {
			fmt.Fprintln(stderr, "symbrain profile remove: unable to safely inspect harness bindings:")
			for _, scanError := range scan.Errors {
				fmt.Fprintf(stderr, "  - %s (%s): %s\n", scanError.Harness, scanError.Path, scanError.Error)
			}
		}
		if len(scan.Bindings) > 0 {
			fmt.Fprintf(stderr, "symbrain profile remove: profile %q is still bound to harnesses:\n", name)
			for _, binding := range scan.Bindings {
				fmt.Fprintf(stderr, "  - %s (%s)\n", binding.Harness, binding.Path)
			}
		}
		fmt.Fprintln(stderr, "Refusing to remove. Use --force to override.")
		return exitcodes.ExitGeneric
	}

	if !*force {
		if _, err := fmt.Fprintf(stdout, "Remove profile %q (%s)? [y/N]: ", name, profile.Path(name)); err != nil {
			return exitcodes.ExitGeneric
		}
		answer, err := readProfileConfirmation(confirmReader)
		if err != nil {
			fmt.Fprintf(stderr, "symbrain profile remove: %s\n", err)
			return exitcodes.ExitGeneric
		}
		if answer != "y" && answer != "yes" {
			fmt.Fprintln(stdout, "aborted")
			return exitcodes.ExitOK
		}
	}

	if err := profile.Remove(name); err != nil {
		fmt.Fprintf(stderr, "symbrain profile remove: %v\n", err)
		return exitcodes.ExitGeneric
	}

	fmt.Fprintf(stdout, "removed %s\n", profile.Path(name))
	return exitcodes.ExitOK
}
