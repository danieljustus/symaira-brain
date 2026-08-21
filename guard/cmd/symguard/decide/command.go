// Package decide implements the `symguard decide` command: the external
// classifier decision interface (issue #81, plan X-5).
//
// # Interface contract
//
// An external caller — first consumer: symbrowse (plan B-48) — sends
// exactly one JSON request on stdin and receives exactly one JSON
// response on stdout:
//
//	request:  {"command": "...", "risk_class": "low|medium|high|critical",
//	           "domain": "...", "warnings": ["..."]}
//	response: {"decision": "allow|confirm|deny", "reason": "..."}
//
// decision is one of allow (proceed), confirm (ask the human), or deny
// (block); reason is a deterministic, human-readable justification that
// is always present. The optional request field "deadline" (RFC 3339)
// bounds the request: once it has passed, the request is expired and
// answered deny, never with a stale decision.
//
// The process boundary is the whole interface: external tools detect the
// guard at runtime with exec.LookPath("symguard") and invoke the binary
// per decision, so there is no compile-time import and no shared state.
//
// # Fail-closed contract
//
// Every error path — missing input, unparseable JSON, missing command,
// unknown risk class, expired deadline, audit-record failure — is mapped
// through model.NewNoDecision (the issue #87 fail-closed contract, see
// internal/model) and answered with decision=deny and an explanatory
// reason. The command always resolves errors to deny; a caller that
// cannot find the binary, cannot send a request, or cannot parse the
// response MUST treat that as deny as well. The process exits 0 whenever
// a JSON decision was written (a deny is a valid decision); it exits 1
// only when even the response could not be written.
//
// # Built-in policy
//
// No config file or rule catalog is consulted in this phase; the
// evaluation is a small deterministic policy over the request fields:
//
//	risk class    warnings      domain            decision
//	low           none          any               allow
//	low           present       any               confirm
//	medium        none          any               allow
//	medium        present       any               confirm
//	high          none          any               confirm
//	high          present       any               deny
//	critical      none          loopback          allow
//	critical      otherwise     otherwise         deny
//	anything else (empty, unknown risk class)      deny
//
// "loopback" is an exact, case-insensitive match against localhost,
// 127.0.0.1, ::1, or 0.0.0.0. Warnings are trimmed; empty warnings are
// dropped before evaluation. A missing command denies. A Phase 3
// enhancement may route the risk class through the config rule catalog
// (internal/policy) when one is configured; the built-in table above
// remains the no-config default.
//
// # Audit record
//
// Every produced decision — including deny-on-error outcomes — is
// recorded as an internal/audit ExternalDecision through a Sink. The
// default sink appends JSON lines to the XDG audit log
// ($XDG_DATA_HOME/symguard/audit.log, directory 0700, file 0600). A
// sink failure on a would-be allow/confirm flips the response to deny
// (fail closed); on an already-deny path the deny reason is kept. Phase 3
// wiring replaces the file sink with the hash-chained sink built on the
// internal/audit chain primitives.
package decide

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/danieljustus/symaira-guard/internal/audit"
	"github.com/danieljustus/symaira-guard/internal/config"
	"github.com/danieljustus/symaira-guard/internal/model"
)

// request is the wire format of one external decision request.
type request struct {
	Command   string    `json:"command"`
	RiskClass string    `json:"risk_class"`
	Domain    string    `json:"domain"`
	Warnings  []string  `json:"warnings"`
	Deadline  time.Time `json:"deadline,omitempty"` // RFC 3339; optional
}

// response is the wire format of one decision response.
type response struct {
	Decision string `json:"decision"` // allow|confirm|deny
	Reason   string `json:"reason"`
}

// outcome is the three-valued decision outcome of the interface.
type outcome string

const (
	allow   outcome = "allow"
	confirm outcome = "confirm"
	deny    outcome = "deny"
)

// loopbackDomains is the allowlist for the critical risk class.
// Exact, case-insensitive match; an empty domain is never allowlisted.
var loopbackDomains = map[string]bool{
	"localhost": true,
	"127.0.0.1": true,
	"::1":       true,
	"0.0.0.0":   true,
}

// Sink receives the audit record for each produced decision.
type Sink interface {
	Write(audit.ExternalDecision) error
}

// Run implements the `symguard decide` command: it reads one JSON
// request from in, writes the JSON decision to out, and records the
// decision through sink. A nil sink uses the default file sink. Every
// error path fails closed to decision=deny with an explanatory reason.
// It returns 0 when a JSON decision was written, 1 when the response
// itself could not be written.
func Run(args []string, in io.Reader, out io.Writer, sink Sink) int {
	if hasHelp(args) {
		printUsage(out)
		return 0
	}
	if sink == nil {
		sink = NewFileSink(defaultAuditPath())
	}

	req, decision, reason := decideRequest(in, time.Now())
	if err := sink.Write(record(req, decision, reason)); err != nil && decision != deny {
		// Audit failure fails closed: a would-be allow/confirm flips to
		// deny. On an already-deny path the primary reason is kept.
		decision, reason = deny, fmt.Sprintf("audit: write decision record: %v", err)
	}
	if err := writeResponse(out, decision, reason); err != nil {
		return 1
	}
	return 0
}

// decideRequest reads one request from in and evaluates it. Every
// failure path returns the fail-closed deny outcome produced by
// model.NewNoDecision; the returned reason is the diagnostic.
func decideRequest(in io.Reader, now time.Time) (request, outcome, string) {
	data, err := io.ReadAll(in)
	if err != nil {
		nd := model.NewNoDecision(model.FailureModeDeny, fmt.Sprintf("decide: read request: %v", err))
		return request{}, deny, nd.Diagnostic
	}
	if len(bytes.TrimSpace(data)) == 0 {
		nd := model.NewNoDecision(model.FailureModeDeny, "decide: empty request")
		return request{}, deny, nd.Diagnostic
	}
	var req request
	if err := json.Unmarshal(data, &req); err != nil {
		nd := model.NewNoDecision(model.FailureModeDeny, fmt.Sprintf("decide: parse request: %v", err))
		return request{}, deny, nd.Diagnostic
	}
	if req.Command == "" {
		nd := model.NewNoDecision(model.FailureModeDeny, "decide: missing command")
		return req, deny, nd.Diagnostic
	}
	if (model.DecisionRequest{Deadline: req.Deadline}).Expired(now) {
		nd := model.NewNoDecision(model.FailureModeDeny, "decide: request deadline expired")
		return req, deny, nd.Diagnostic
	}
	decision, reason := evaluate(req)
	return req, decision, reason
}

// evaluate applies the built-in deterministic policy to a validated
// request. See the package doc for the full mapping table.
func evaluate(req request) (outcome, string) {
	risk := strings.ToLower(strings.TrimSpace(req.RiskClass))
	warnings := cleanWarnings(req.Warnings)
	domain := strings.ToLower(strings.TrimSpace(req.Domain))

	switch risk {
	case "low", "medium":
		if len(warnings) == 0 {
			return allow, fmt.Sprintf("%s risk class, no warnings", risk)
		}
		return confirm, fmt.Sprintf("%s risk class with warnings: %s", risk, strings.Join(warnings, "; "))
	case "high":
		if len(warnings) == 0 {
			return confirm, "high risk class requires confirmation"
		}
		return deny, fmt.Sprintf("high risk class with warnings: %s", strings.Join(warnings, "; "))
	case "critical":
		if len(warnings) == 0 && loopbackDomains[domain] {
			return allow, fmt.Sprintf("critical risk class on allowlisted domain %q, no warnings", req.Domain)
		}
		return deny, "critical risk class: requires allowlisted domain and no warnings"
	default:
		return deny, fmt.Sprintf("decide: unknown risk class %q", req.RiskClass)
	}
}

// cleanWarnings trims warning strings and drops empty ones, preserving order.
func cleanWarnings(in []string) []string {
	out := make([]string, 0, len(in))
	for _, w := range in {
		if w = strings.TrimSpace(w); w != "" {
			out = append(out, w)
		}
	}
	return out
}

// record builds the audit record for a produced decision. The ID and
// timestamp are generated per decision; the request fields are recorded
// as received, unredacted.
func record(req request, decision outcome, reason string) audit.ExternalDecision {
	now := time.Now().UTC()
	return audit.ExternalDecision{
		ID:        model.EventID(model.SourceDecide, now.UnixNano()),
		Command:   req.Command,
		RiskClass: req.RiskClass,
		Domain:    req.Domain,
		Warnings:  req.Warnings,
		Decision:  string(decision),
		Reason:    reason,
		DecidedAt: now.Format(time.RFC3339),
	}
}

// writeResponse writes the JSON decision response to out.
func writeResponse(out io.Writer, decision outcome, reason string) error {
	return json.NewEncoder(out).Encode(response{Decision: string(decision), Reason: reason})
}

// FileSink appends one JSON line per decision record to a log file.
// The directory is created with 0700 and the file with 0600; each
// record is a single portable JSON line (RFC 3339 timestamps, snake_case
// keys) suitable for the Phase 3 hash-chained log.
type FileSink struct {
	path string
}

// NewFileSink returns a Sink that appends records to the given path.
func NewFileSink(path string) *FileSink {
	return &FileSink{path: path}
}

// Write appends one JSON line per record to the log file.
func (s *FileSink) Write(rec audit.ExternalDecision) error {
	data, err := json.Marshal(rec)
	if err != nil {
		return fmt.Errorf("decide: marshal audit record: %w", err)
	}
	if err := os.MkdirAll(filepath.Dir(s.path), 0700); err != nil {
		return fmt.Errorf("decide: mkdir audit dir: %w", err)
	}
	f, err := os.OpenFile(s.path, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0600)
	if err != nil {
		return fmt.Errorf("decide: open audit log: %w", err)
	}
	defer f.Close()
	if _, err := f.Write(append(data, '\n')); err != nil {
		return fmt.Errorf("decide: write audit log: %w", err)
	}
	return nil
}

// defaultAuditPath returns the XDG audit log path for the default sink.
func defaultAuditPath() string {
	return filepath.Join(config.DataDir(), "audit.log")
}

// hasHelp reports whether args request help output.
func hasHelp(args []string) bool {
	for _, a := range args {
		if a == "--help" || a == "-h" {
			return true
		}
	}
	return false
}

// printUsage writes the command's usage text to w.
func printUsage(w io.Writer) {
	fmt.Fprintln(w, `Usage:
  symguard decide < request.json

Reads one JSON decision request from stdin and writes the JSON decision
to stdout:

  request:  {"command": "...", "risk_class": "low|medium|high|critical",
             "domain": "...", "warnings": ["..."]}
  response: {"decision": "allow|confirm|deny", "reason": "..."}

Any error produces decision "deny" with an explanatory reason.`)
}
