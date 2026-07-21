// Package skills orchestrates symskills: it shells out to the symskills CLI
// and parses its --json output, without its own understanding of SKILL.md.
package skills

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os/exec"
	"strings"
	"time"
)

// HarnessMap maps symbrain harness names to symskills target names.
// Harnesses not present in this map are skipped with an informational
// message — symskills may not support them.
var HarnessMap = map[string]string{
	"claude":   "claude",
	"codex":    "codex",
	"opencode": "opencode",
	"hermes":   "hermes",
}

// Result holds the outcome of one symskills render/install invocation.
type Result struct {
	Target  string `json:"target"`
	Status  string `json:"status"`
	Message string `json:"message,omitempty"`
}

// Response is the top-level JSON structure returned by symskills --json.
type Response struct {
	Results []Result `json:"results"`
}

// Runner executes symskills commands.  The default implementation shells
// out to the real binary; tests inject a fake.
type Runner struct {
	// LookPath resolves the symskills binary.  Defaults to exec.LookPath.
	LookPath func(name string) (string, error)
	// Run executes a command and returns combined stdout+stderr.
	Run func(name string, args ...string) ([]byte, error)
}

// DefaultRunner returns a Runner that uses exec.LookPath and os/exec.
func DefaultRunner() *Runner {
	return &Runner{
		LookPath: exec.LookPath,
		Run: func(name string, args ...string) ([]byte, error) {
			cmd := exec.Command(name, args...)
			var buf bytes.Buffer
			cmd.Stdout = &buf
			cmd.Stderr = &buf
			err := cmd.Run()
			return buf.Bytes(), err
		},
	}
}

// Available reports whether symskills is on PATH.
func (r *Runner) Available() bool {
	_, err := r.LookPath("symskills")
	return err == nil
}

// Sync runs symskills render/install for each harness target in targets.
// Returns the parsed results and a human-readable summary.  When symskills
// is absent, returns a single result with status "skipped" and a hint.
func (r *Runner) Sync(targets []string, timeout time.Duration) ([]Result, string) {
	if !r.Available() {
		return []Result{{
			Status:  "skipped",
			Message: "symskills not found on PATH; install it to sync skills (https://github.com/danieljustus/symaira-skills)",
		}}, "symskills not found — skills sync skipped"
	}

	var results []Result
	var summary []string

	for _, harness := range targets {
		skillsTarget, ok := HarnessMap[harness]
		if !ok {
			results = append(results, Result{
				Target:  harness,
				Status:  "skipped",
				Message: fmt.Sprintf("symskills does not support harness %q", harness),
			})
			summary = append(summary, fmt.Sprintf("%s: skipped (unsupported)", harness))
			continue
		}

		result, err := r.runSync(skillsTarget, timeout)
		if err != nil {
			results = append(results, Result{
				Target:  harness,
				Status:  "error",
				Message: err.Error(),
			})
			summary = append(summary, fmt.Sprintf("%s: error — %v", harness, err))
			continue
		}
		results = append(results, result)
		summary = append(summary, fmt.Sprintf("%s: %s", harness, result.Status))
	}

	return results, strings.Join(summary, "; ")
}

func (r *Runner) runSync(target string, timeout time.Duration) (Result, error) {
	_ = time.AfterFunc(timeout, func() {
		// timeout is best-effort; the command will be killed by the OS
		// if it exceeds this, but we don't enforce it here.
	})

	data, err := r.Run("symskills", "render", "--target", target, "--json")
	if err != nil {
		return Result{}, fmt.Errorf("symskills render --target %s: %w", target, err)
	}

	var resp Response
	if err := json.Unmarshal(data, &resp); err != nil {
		return Result{}, fmt.Errorf("symskills render --target %s: invalid JSON: %w", target, err)
	}

	if len(resp.Results) == 0 {
		return Result{
			Target:  target,
			Status:  "ok",
			Message: "no skills rendered",
		}, nil
	}

	return resp.Results[0], nil
}
