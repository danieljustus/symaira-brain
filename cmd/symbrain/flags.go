package main

import (
	"strings"

	"github.com/danieljustus/symaira-brain/internal/output"
)

// normalizeFlags normalizes CLI arguments for Go's flag package.
// It converts double-dash flags ("--flag") to single-dash ("-flag")
// so FlagSet handles them uniformly, while leaving positionals,
// bare "-" (stdin), and terminator "--" (and everything after it) intact.
func normalizeFlags(args []string) []string {
	out := make([]string, len(args))
	terminated := false
	for i, arg := range args {
		if terminated {
			out[i] = arg
			continue
		}
		if arg == "--" {
			terminated = true
			out[i] = arg
			continue
		}
		if strings.HasPrefix(arg, "--") && len(arg) > 2 {
			out[i] = "-" + strings.TrimPrefix(arg, "--")
		} else {
			out[i] = arg
		}
	}
	return out
}

// extractFormat normalizes single-dash output flags (-json, -output) to
// double-dash (--json, --output) and delegates to output.Extract.
func extractFormat(args []string) (output.Format, []string, error) {
	extractedArgs := make([]string, len(args))
	for i, a := range args {
		switch {
		case a == "-json":
			extractedArgs[i] = "--json"
		case a == "-output":
			extractedArgs[i] = "--output"
		case strings.HasPrefix(a, "-output="):
			extractedArgs[i] = "--output=" + strings.TrimPrefix(a, "-output=")
		default:
			extractedArgs[i] = a
		}
	}
	return output.Extract(extractedArgs)
}
