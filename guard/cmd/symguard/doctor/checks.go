package doctor

import (
	"fmt"
	"io"
	"sort"
	"strings"

	"github.com/danieljustus/symaira-guard/internal/discovery"
	"github.com/danieljustus/symaira-guard/internal/spawn"
)

// ServerCheck is the doctor verdict for one discovered MCP server.
type ServerCheck struct {
	// Name is the server's key in the client config.
	Name string

	// Client is the originating AI client.
	Client discovery.Client

	// Command is the executable or URL as configured.
	Command string

	// Args are the server's arguments (stdio only).
	Args []string

	// Transport is the server's transport.
	Transport discovery.Transport

	// Allowed reports whether the launch is allowlisted. Stdio servers not
	// on the allowlist are denied; HTTP servers are not gated (no executable
	// is spawned) and report Allowed=true.
	Allowed bool

	// Secrets lists env keys whose values are stored as plaintext in the
	// client config and look like secrets.
	Secrets []string
}

// checkServers evaluates discovered servers against the spawn allowlist and
// flags plaintext secret env values. Results are sorted by client and name
// for stable output.
func checkServers(servers []discovery.Server, al *spawn.Allowlist) []ServerCheck {
	checks := make([]ServerCheck, 0, len(servers))
	for _, s := range servers {
		checks = append(checks, ServerCheck{
			Name:      s.Name,
			Client:    s.Client,
			Command:   s.Command,
			Args:      s.Args,
			Transport: s.Transport,
			Allowed:   al.Allows(s),
			Secrets:   s.PlaintextSecretKeys(),
		})
	}
	sort.Slice(checks, func(i, j int) bool {
		if checks[i].Client != checks[j].Client {
			return checks[i].Client < checks[j].Client
		}
		return checks[i].Name < checks[j].Name
	})
	return checks
}

// issueCount returns the number of servers that are denied by the spawn
// allowlist or carry plaintext secrets.
func issueCount(checks []ServerCheck) int {
	n := 0
	for _, c := range checks {
		if !c.Allowed || len(c.Secrets) > 0 {
			n++
		}
	}
	return n
}

// printServerChecks writes the per-server spawn verdicts to w.
func printServerChecks(w io.Writer, checks []ServerCheck) {
	if len(checks) == 0 {
		return
	}
	fmt.Fprintln(w, "\nDiscovered MCP servers (spawn allowlist):")
	for _, c := range checks {
		verdict := "[allowed]"
		switch {
		case c.Transport == discovery.TransportHTTP:
			verdict = "[n/a]    "
		case !c.Allowed:
			verdict = "[DENIED] "
		}
		desc := fmt.Sprintf("%s (%s/%s) → %s", c.Name, c.Client, c.Transport, c.Command)
		if len(c.Args) > 0 {
			desc += " " + strings.Join(c.Args, " ")
		}
		if !c.Allowed && c.Transport == discovery.TransportStdio {
			desc += " (not on spawn allowlist)"
		}
		fmt.Fprintf(w, "  %s %s\n", verdict, desc)
	}
}

// printSecretRisks writes plaintext-secret findings and the resolution
// boundary to w.
func printSecretRisks(w io.Writer, checks []ServerCheck) {
	any := false
	for _, c := range checks {
		if len(c.Secrets) == 0 {
			continue
		}
		if !any {
			any = true
			fmt.Fprintln(w, "\nPlaintext secret risk:")
		}
		fmt.Fprintf(w, "  %s (%s): env %s stored as plaintext values in the client config\n",
			c.Name, c.Client, strings.Join(c.Secrets, ", "))
	}
	if any {
		fmt.Fprintln(w, "  symguard reports this risk but is not a secret store — move these values to symvault and reference them at launch time.")
	}
}
