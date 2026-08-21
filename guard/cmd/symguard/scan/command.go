// Package scan implements the `symguard scan` command.
//
// It discovers MCP servers across supported AI clients and writes the
// machine-readable inventory to stdout while reporting findings to stderr,
// following the direction already established in internal/output.
package scan

import (
	"fmt"
	"io"
	"sort"
	"strings"

	"github.com/danieljustus/symaira-guard/internal/discovery"
	"github.com/danieljustus/symaira-guard/internal/output"
)

// discoverAll is the discovery entry point used by Run. Tests override it to
// avoid touching real client configs.
var discoverAll = func() discovery.Result { return discovery.ScanAll() }

// Run scans for MCP servers across supported AI clients. The inventory is
// written to out; findings are written to errOut. It returns 0 on success
// and 1 on usage errors.
func Run(args []string, out, errOut io.Writer) int {
	format, code, done := parseFlags(args, out, errOut)
	if done {
		return code
	}
	if format == "" {
		format = output.Resolve("")
	}
	if format != "table" && format != "json" {
		fmt.Fprintf(errOut, "scan: unsupported format %q (supported: %s)\n", format, strings.Join(output.FormatNames(), ", "))
		return 1
	}

	res := discoverAll()
	inv := inventory{Servers: toViews(res.Servers)}
	if err := output.NewReporter(format).Print(out, inv); err != nil {
		fmt.Fprintf(errOut, "scan: write output: %v\n", err)
		return 1
	}
	writeFindings(errOut, res.Findings)
	return 0
}

// parseFlags parses scan flags. It returns the requested format, an exit
// code, and whether processing is already complete (help or an error).
func parseFlags(args []string, out, errOut io.Writer) (format string, code int, done bool) {
	for i := 0; i < len(args); i++ {
		switch a := args[i]; {
		case a == "--format":
			if i+1 >= len(args) {
				fmt.Fprintln(errOut, "scan: --format requires a value (table or json)")
				return "", 1, true
			}
			i++
			format = args[i]
		case strings.HasPrefix(a, "--format="):
			format = strings.TrimPrefix(a, "--format=")
		case a == "--help" || a == "-h":
			usage(out)
			return "", 0, true
		default:
			fmt.Fprintf(errOut, "scan: unknown argument %q\n", a)
			usage(errOut)
			return "", 1, true
		}
	}
	return format, 0, false
}

// inventory is the machine-readable scan result written to stdout. Server
// entries are redacted views; environment values are never included.
type inventory struct {
	Servers []serverView `json:"servers"`
}

// String renders the inventory as human-readable table lines.
func (i inventory) String() string {
	var b strings.Builder
	fmt.Fprintf(&b, "Discovered %d MCP server(s)\n", len(i.Servers))
	for _, s := range i.Servers {
		fmt.Fprintf(&b, "  %s\n", s.line())
	}
	return strings.TrimSuffix(b.String(), "\n")
}

// serverView is a JSON-safe rendering of a discovered server with all
// environment values replaced by redacted placeholders.
type serverView struct {
	Name      string              `json:"name"`
	Client    discovery.Client    `json:"client"`
	Command   string              `json:"command"`
	Args      []string            `json:"args,omitempty"`
	Env       map[string]string   `json:"env,omitempty"`
	Transport discovery.Transport `json:"transport"`
}

// line renders a single human-readable inventory line.
func (v serverView) line() string {
	var b strings.Builder
	fmt.Fprintf(&b, "%s (%s/%s)", v.Name, v.Client, v.Transport)
	if v.Command != "" {
		fmt.Fprintf(&b, " → %s", v.Command)
	}
	if len(v.Args) > 0 {
		fmt.Fprintf(&b, " %s", strings.Join(v.Args, " "))
	}
	return b.String()
}

// toViews converts discovered servers into redacted views, sorted by client
// and name for deterministic output.
func toViews(servers []discovery.Server) []serverView {
	sorted := append([]discovery.Server(nil), servers...)
	sort.Slice(sorted, func(i, j int) bool {
		if sorted[i].Client != sorted[j].Client {
			return sorted[i].Client < sorted[j].Client
		}
		return sorted[i].Name < sorted[j].Name
	})
	views := make([]serverView, 0, len(sorted))
	for _, s := range sorted {
		views = append(views, serverView{
			Name:      s.Name,
			Client:    s.Client,
			Command:   s.Command,
			Args:      s.Args,
			Env:       s.RedactedEnv(),
			Transport: s.Transport,
		})
	}
	return views
}

// writeFindings reports scan findings to errOut. Nothing is written when
// there are no findings.
func writeFindings(errOut io.Writer, findings []discovery.Finding) {
	if len(findings) == 0 {
		return
	}
	fmt.Fprintf(errOut, "symguard scan: %d finding(s)\n", len(findings))
	for _, f := range findings {
		fmt.Fprintf(errOut, "  %s [%s] %s: %s\n", f.Status, f.Client, f.Path, f.Message)
	}
}

// usage prints the scan command help text.
func usage(w io.Writer) {
	fmt.Fprintln(w, `Usage:
  symguard scan [--format table|json]

Discovers MCP servers across supported AI clients (hermes, claude-desktop,
cursor, vscode, opencode). The inventory is written to stdout; findings —
clients or entries that could not be mapped — are written to stderr.`)
}
