// Package output centralizes symbrain's command output format selection and rendering.
package output

import (
	"encoding/json"
	"fmt"
	"io"
	"strings"
)

// Format selects the representation written to stdout.
type Format string

const (
	// Table is the stable human-readable default. It is intentionally not
	// TTY-sensitive so redirecting stdout does not change existing output.
	FormatTable Format = "table"
	// JSON is the machine-readable representation.
	FormatJSON Format = "json"

	// Short aliases keep call sites readable while preserving the explicit
	// Format-prefixed names for callers that prefer them.
	Table = FormatTable
	JSON  = FormatJSON
)

// Rows contains the representations of one command result. JSON is encoded
// by this package, while Table supplies the command-specific human-readable
// layout. Render is the only place that chooses between the two.
type Rows struct {
	JSON  any
	Table func(io.Writer) error
}

// Resolve returns the requested format, defaulting explicitly to table for an
// empty or unknown value. Use Parse when invalid user input must be rejected.
func Resolve(explicit string) Format {
	if strings.EqualFold(strings.TrimSpace(explicit), string(FormatJSON)) {
		return FormatJSON
	}
	return FormatTable
}

// Parse validates an explicit format value. An empty value resolves to table.
func Parse(explicit string) (Format, error) {
	switch strings.ToLower(strings.TrimSpace(explicit)) {
	case "":
		return FormatTable, nil
	case string(FormatTable):
		return FormatTable, nil
	case string(FormatJSON):
		return FormatJSON, nil
	default:
		return "", fmt.Errorf("unsupported output format %q (want table or json)", explicit)
	}
}

// Extract removes global output flags from args and resolves their format.
// Both --output table|json and the legacy --json shorthand are accepted at any
// position, including between a subcommand and its positional arguments.
func Extract(args []string) (Format, []string, error) {
	explicit := ""
	cleaned := make([]string, 0, len(args))
	for i := 0; i < len(args); i++ {
		arg := args[i]
		switch {
		case arg == "--json":
			if err := setExplicit(&explicit, string(FormatJSON)); err != nil {
				return FormatTable, cleaned, err
			}
		case arg == "--output" || arg == "--format":
			if i+1 >= len(args) {
				return FormatTable, cleaned, fmt.Errorf("%s requires a value", arg)
			}
			i++
			if err := setExplicit(&explicit, args[i]); err != nil {
				return FormatTable, cleaned, err
			}
		case strings.HasPrefix(arg, "--output="):
			if err := setExplicit(&explicit, strings.TrimPrefix(arg, "--output=")); err != nil {
				return FormatTable, cleaned, err
			}
		case strings.HasPrefix(arg, "--format="):
			if err := setExplicit(&explicit, strings.TrimPrefix(arg, "--format=")); err != nil {
				return FormatTable, cleaned, err
			}
		default:
			cleaned = append(cleaned, arg)
		}
	}

	format, err := Parse(explicit)
	return format, cleaned, err
}

func setExplicit(current *string, next string) error {
	if _, err := Parse(next); err != nil {
		return err
	}
	if *current != "" && !strings.EqualFold(*current, next) {
		return fmt.Errorf("conflicting output formats %q and %q", *current, next)
	}
	*current = next
	return nil
}

// Render writes rows in the selected format. For JSON, rows may be any value;
// when Rows is supplied its JSON field is encoded. For table output, Rows.Table
// is used, while simple values are written with fmt.Fprintln.
func Render(w io.Writer, format Format, rows any) error {
	switch format {
	case FormatJSON:
		if packaged, ok := rows.(Rows); ok {
			rows = packaged.JSON
		}
		return json.NewEncoder(w).Encode(rows)
	case FormatTable:
		if packaged, ok := rows.(Rows); ok {
			if packaged.Table == nil {
				return nil
			}
			return packaged.Table(w)
		}
		_, err := fmt.Fprintln(w, rows)
		return err
	default:
		return fmt.Errorf("unsupported output format %q", format)
	}
}
