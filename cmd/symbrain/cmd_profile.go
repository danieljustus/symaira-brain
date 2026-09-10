package main

import (
	"bufio"
	"errors"
	"fmt"
	"io"
	"os"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/output"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// confirmReader is read for the `profile remove` confirmation prompt.
// Overridable in tests so they never block on real stdin.
var confirmReader io.Reader = os.Stdin

const maxProfileConfirmationBytes = 1024

func readProfileConfirmation(reader io.Reader) (string, error) {
	line, err := bufio.NewReader(io.LimitReader(reader, maxProfileConfirmationBytes+2)).ReadString('\n')
	if err != nil && !errors.Is(err, io.EOF) {
		return "", fmt.Errorf("unable to read confirmation input")
	}
	if len(line) > maxProfileConfirmationBytes+1 ||
		(len(line) == maxProfileConfirmationBytes+1 && !strings.HasSuffix(line, "\n")) {
		return "", fmt.Errorf("confirmation input exceeds maximum of %d bytes", maxProfileConfirmationBytes)
	}
	return strings.ToLower(strings.TrimSpace(line)), nil
}

func cmdProfile(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	format, args, err := extractFormat(args)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain profile: %v\n", err)
		return exitcodes.ExitNoInput
	}
	return cmdProfileWithFormat(args, stdout, stderr, format)
}

func cmdProfileWithFormat(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	if len(args) < 1 {
		printProfileUsage(stderr)
		return exitcodes.ExitNoInput
	}

	sub, rest := args[0], args[1:]
	switch sub {
	case "list":
		return cmdProfileListWithFormat(rest, stdout, stderr, format)
	case "show":
		return cmdProfileShowWithFormat(rest, stdout, stderr, format)
	case "add":
		return cmdProfileAdd(rest, stdout, stderr)
	case "remove":
		return cmdProfileRemove(rest, stdout, stderr)
	case "help", "--help", "-h":
		printProfileUsage(stdout)
		return exitcodes.ExitOK
	default:
		fmt.Fprintf(stderr, "symbrain profile: unknown subcommand %q\n\n", sub)
		printProfileUsage(stderr)
		return exitcodes.ExitNoInput
	}
}

// reorderFlagsFirst moves recognized long ("--flag") flags — and, for the
// names listed in valueFlags, their following value — to the front of args,
// leaving positional arguments after them in their original relative order.
// It is used by profile add/remove for their command-local flags; output
// formatting is handled globally by internal/output.
func reorderFlagsFirst(args []string, valueFlags map[string]bool) []string {
	var flags, positionals []string
	for i := 0; i < len(args); i++ {
		a := args[i]
		if !strings.HasPrefix(a, "-") || a == "-" || a == "--" {
			positionals = append(positionals, a)
			continue
		}
		flags = append(flags, a)
		name := strings.TrimPrefix(strings.TrimPrefix(a, "--"), "-")
		if valueFlags[name] && i+1 < len(args) {
			i++
			flags = append(flags, args[i])
		}
	}
	return append(flags, positionals...)
}
