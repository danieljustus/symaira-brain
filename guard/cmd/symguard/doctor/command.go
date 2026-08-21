// Package doctor implements the `symguard doctor` command.
//
// Doctor reports system health and configuration information, including the
// spawn allowlist gate for discovered stdio MCP servers and plaintext-secret
// risks in their configs. It reports and gates; it never becomes a secret
// store — resolution stays with symvault.
package doctor

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"

	"github.com/danieljustus/symaira-guard/internal/audit"
	"github.com/danieljustus/symaira-guard/internal/config"
	"github.com/danieljustus/symaira-guard/internal/discovery"
	"github.com/danieljustus/symaira-guard/internal/spawn"
)

// Run prints system health and configuration information to w and returns
// an exit code: 0 when every check passed, 1 when any check reported an
// issue or errored.
func Run(w io.Writer) int {
	fmt.Fprintln(w, "symguard doctor")
	fmt.Fprintln(w)
	fmt.Fprintf(w, "  Version:   %s\n", versionInfo())
	fmt.Fprintf(w, "  Go:        %s\n", runtime.Version())
	fmt.Fprintf(w, "  OS/Arch:   %s/%s\n", runtime.GOOS, runtime.GOARCH)
	fmt.Fprintln(w)

	fmt.Fprintf(w, "  %-16s %s\n", "binary", "ok")
	fmt.Fprintf(w, "  %-16s %s\n", "go runtime", "ok")

	problems := 0
	report := func(name, status string, isIssue bool) {
		if isIssue {
			problems++
		}
		fmt.Fprintf(w, "  %-16s %s\n", name, status)
	}

	// Config: reflect the actual load result. A missing file is normal and
	// resolves to fail-closed defaults; a file that exists but does not
	// parse is a real problem.
	cfg, err := config.Load()
	if _, statErr := os.Stat(config.ConfigPath()); os.IsNotExist(statErr) {
		report("config", "not configured (no config file found)", false)
	} else if err != nil {
		report("config", fmt.Sprintf("error: %v", err), true)
	} else {
		report("config", "ok", false)
	}

	// Policy: with a healthy config the rule count is the real state; with
	// a config error policy cannot be loaded. An empty rule list still
	// enforces the fail-closed defaults, so it is not an issue.
	switch {
	case err != nil:
		report("policy", "not loaded (config error)", true)
	case len(cfg.Rules) > 0:
		report("policy", fmt.Sprintf("ok (%d rule(s))", len(cfg.Rules)), false)
	default:
		report("policy", "defaults only (no rules — deny by default)", false)
	}

	// Audit log: probe the XDG data path. A missing log is normal until the
	// first `symguard decide` call creates it. decide's current FileSink
	// writes plain JSON lines without a chain anchor (the hash-chained sink
	// is Phase 3 wiring, see cmd/symguard/decide), so a missing anchor is
	// the expected state today and not an issue. An anchor that exists but
	// cannot be read or parsed breaks truncation detection and is a problem.
	logPath := defaultAuditLogPath()
	anchorPath := audit.DefaultAnchorPath(logPath)
	switch _, statErr := os.Stat(logPath); {
	case os.IsNotExist(statErr):
		report("audit log", "not initialized (created on first 'symguard decide')", false)
	case statErr != nil:
		report("audit log", fmt.Sprintf("error: %v", statErr), true)
	default:
		switch anchor, err := audit.ReadCheckpoint(anchorPath); {
		case err != nil:
			report("audit log", fmt.Sprintf("error: anchor %s: %v", anchorPath, err), true)
		case anchor == nil:
			report("audit log", "ok (JSONL, chain anchor pending Phase 3 sink)", false)
		default:
			report("audit log", "ok (hash-chained, anchor present)", false)
		}
	}

	// Spawn allowlist and discovered MCP servers: doctor reports and gates,
	// it does not become a secret store.
	var serverChecks []ServerCheck
	if err == nil {
		allowlist := spawn.NewAllowlist(cfg.Spawn.Allowlist)
		if allowlist.Len() == 0 {
			report("spawn allowlist", "not configured (empty — deny by default)", false)
		} else {
			report("spawn allowlist", fmt.Sprintf("ok (%d entries)", allowlist.Len()), false)
		}

		servers, discErr := discovery.DiscoverAll()
		switch {
		case discErr != nil:
			report("mcp servers", fmt.Sprintf("error: %v", discErr), true)
		case len(servers) == 0:
			report("mcp servers", "none discovered", false)
		default:
			report("mcp servers", fmt.Sprintf("%d discovered", len(servers)), false)
			serverChecks = checkServers(servers, allowlist)
			problems += issueCount(serverChecks)
		}
	}

	printServerChecks(w, serverChecks)
	printSecretRisks(w, serverChecks)

	fmt.Fprintln(w)
	if problems == 0 {
		fmt.Fprintln(w, "All basic checks passed. Run 'symguard scan' after setup for full diagnostics.")
		return 0
	}
	fmt.Fprintf(w, "%d issue(s) found. See details above.\n", problems)
	return 1
}

// defaultAuditLogPath returns the XDG data path for the audit log. It
// mirrors the decide command's default sink so doctor probes the same file
// the rest of symguard writes.
func defaultAuditLogPath() string {
	return filepath.Join(config.DataDir(), "audit.log")
}

// versionInfo returns the version string for display.
var versionInfo = func() string { return "dev" }

// SetVersion sets the version callback for the doctor command.
func SetVersion(fn func() string) {
	versionInfo = fn
}
