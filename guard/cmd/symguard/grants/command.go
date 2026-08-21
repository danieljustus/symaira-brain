// Package grants implements the `symguard grants` command: listing and
// revoking standing grants. It is the kill-switch surface for the grant
// store (internal/grant) — the same set the settings surface enumerates,
// so the two can never drift apart.
package grants

import (
	"fmt"
	"io"
	"strings"
	"text/tabwriter"
	"time"

	"github.com/danieljustus/symaira-guard/internal/grant"
)

// Run executes `symguard grants <list|revoke> ...` writing to w.
func Run(args []string, w io.Writer) {
	if len(args) == 0 {
		printUsage(w)
		return
	}
	switch args[0] {
	case "list":
		runList(w)
	case "revoke":
		runRevoke(args[1:], w)
	case "help", "--help", "-h":
		printUsage(w)
	default:
		fmt.Fprintf(w, "unknown grants subcommand: %s\n\n", args[0])
		printUsage(w)
	}
}

// runList prints every active grant.
func runList(w io.Writer) {
	st, err := grant.Open(grant.DefaultDir())
	if err != nil {
		fmt.Fprintf(w, "grants: %v\n", err)
		return
	}
	active := st.Active()
	if len(active) == 0 {
		fmt.Fprintln(w, "No active grants.")
		return
	}
	tw := tabwriter.NewWriter(w, 2, 4, 2, ' ', 0)
	fmt.Fprintln(tw, "ID\tSCOPE\tSUBJECT\tORIGIN\tGRANTED_AT")
	for _, g := range active {
		fmt.Fprintf(tw, "%s\t%s\t%s\t%s@%d\t%s\n",
			g.ID, g.Scope, g.Subject, g.Origin.Via, g.Origin.Epoch,
			g.GrantedAt.UTC().Format(time.RFC3339))
	}
	tw.Flush()
}

// runRevoke revokes one grant by ID or every active grant with --all.
func runRevoke(args []string, w io.Writer) {
	all := false
	var id string
	for _, a := range args {
		switch {
		case a == "--all":
			all = true
		case a == "--help" || a == "-h":
			printUsage(w)
			return
		case strings.HasPrefix(a, "-"):
			fmt.Fprintf(w, "grants revoke: unexpected flag %q\n", a)
			printUsage(w)
			return
		case id == "":
			id = a
		default:
			fmt.Fprintf(w, "grants revoke: unexpected argument %q\n", a)
			printUsage(w)
			return
		}
	}
	if all && id != "" {
		fmt.Fprintln(w, "grants revoke: --all cannot be combined with a grant ID")
		printUsage(w)
		return
	}

	st, err := grant.Open(grant.DefaultDir())
	if err != nil {
		fmt.Fprintf(w, "grants: %v\n", err)
		return
	}
	if all {
		n, err := st.RevokeAll()
		if err != nil {
			fmt.Fprintf(w, "grants: %v\n", err)
			return
		}
		fmt.Fprintf(w, "Revoked %d grant(s).\n", n)
		return
	}
	if id == "" {
		fmt.Fprintln(w, "grants revoke: missing grant ID or --all")
		printUsage(w)
		return
	}
	if err := st.Revoke(id); err != nil {
		fmt.Fprintf(w, "grants: %v\n", err)
		return
	}
	fmt.Fprintf(w, "Revoked grant %s.\n", id)
}

func printUsage(w io.Writer) {
	fmt.Fprintln(w, `Usage:
  symguard grants list
  symguard grants revoke <id> | --all

Commands:
  list      List active grants
  revoke    Revoke a grant by ID, or revoke every grant with --all

Run 'symguard grants <command> --help' for details.`)
}
