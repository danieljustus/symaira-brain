// Package output provides a Reporter interface and format-specific
// implementations for rendering symguard results.
//
// Supported formats:
//   - "table" (default): human-readable tabular output
//   - "json": JSON-formatted output
//
// The package has no dependency on scan, discovery, or other subsystem
// internals — it only knows how to render a result value it's given.
package output

import (
	"encoding/json"
	"fmt"
	"io"
	"os"

	"golang.org/x/term"
)

// Reporter renders a result value to the given writer.
type Reporter interface {
	// Print renders the result to w and returns any error.
	Print(w io.Writer, result any) error
}

// NewReporter returns a Reporter for the given format string.
// Supported formats: "table" (default), "json".
func NewReporter(format string) Reporter {
	return newReporter(resolveFormat(format))
}

func newReporter(format string) Reporter {
	switch format {
	case "json":
		return &jsonReporter{}
	case "table", "":
		return &tableReporter{}
	default:
		return &tableReporter{}
	}
}

// Resolve returns the effective format to use.
// If explicit is set, it is returned as-is.
// If empty, "table" is used when stdout is a terminal, "json" when piped.
func Resolve(explicit string) string {
	if explicit != "" {
		return explicit
	}
	if isTerminal(os.Stdout) {
		return "table"
	}
	return "json"
}

func resolveFormat(explicit string) string {
	return Resolve(explicit)
}

// isTerminal reports whether w is a terminal file descriptor.
var isTerminal = func(w *os.File) bool {
	return term.IsTerminal(int(w.Fd()))
}

// SetTerminalCheck allows tests to override the terminal detection.
func SetTerminalCheck(fn func(*os.File) bool) {
	isTerminal = fn
}

type jsonReporter struct{}

func (r *jsonReporter) Print(w io.Writer, result any) error {
	enc := json.NewEncoder(w)
	enc.SetIndent("", "  ")
	return enc.Encode(result)
}

type tableReporter struct{}

func (r *tableReporter) Print(w io.Writer, result any) error {
	// For structured results, attempt a simple key-value rendering.
	// Future implementations may use richer formatting for known types.
	switch v := result.(type) {
	case string:
		_, err := fmt.Fprintln(w, v)
		return err
	case fmt.Stringer:
		_, err := fmt.Fprintln(w, v.String())
		return err
	default:
		// Fall back to compact JSON for unknown types.
		enc := json.NewEncoder(w)
		enc.SetIndent("", "")
		return enc.Encode(v)
	}
}

// FormatNames returns the list of supported format names.
func FormatNames() []string {
	return []string{"table", "json"}
}
