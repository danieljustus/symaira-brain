// Package sync orchestrates the symbrain sync command: it renders
// instruction targets for configured harnesses and triggers in-process
// skill rendering/install via the skills library.
package sync

import (
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"

	"github.com/danieljustus/symaira-brain/internal/adapter"
	"github.com/danieljustus/symaira-brain/internal/harness"
	"github.com/danieljustus/symaira-brain/internal/instructions"
	"github.com/danieljustus/symaira-brain/internal/output"
	"github.com/danieljustus/symaira-brain/internal/skillsrunner"
)

// TargetStatus is the outcome of syncing one adapter target.
type TargetStatus struct {
	Name    string `json:"name"`
	Path    string `json:"path"`
	Status  string `json:"status"` // created, updated, unchanged, dry-run, skipped, error
	Message string `json:"message,omitempty"`
}

// Run executes the sync operation. It renders instruction targets for the
// specified harnesses, then renders and installs library skills in-process
// through internal/skillsrunner (no external symskills binary). When
// dryRun is true no files are written. Returns every per-target summary; it
// returns an error after collection when an instruction target failed.
func Run(projectDir string, harnessNames []string, dryRun bool, stderr io.Writer) ([]TargetStatus, []skillsrunner.Result, error) {
	if len(harnessNames) == 0 {
		for _, h := range harness.All {
			harnessNames = append(harnessNames, string(h.Name))
		}
	}

	// Resolve instruction source.
	source := instructions.NewSource(projectDir)
	defer source.Close()
	content, err := source.Content()
	if err != nil {
		return nil, nil, fmt.Errorf("sync: load instructions: %w", err)
	}

	// Map registered harness names to adapters. The registry owns the
	// capability relationship; harnesses without an adapter remain valid
	// names and are reported as skipped below.
	supported := adapter.TargetsForHarnesses()

	var statuses []TargetStatus
	for _, name := range harnessNames {
		t, ok := supported[name]
		if !ok {
			statuses = append(statuses, TargetStatus{
				Name:    name,
				Status:  "skipped",
				Message: fmt.Sprintf("harness %q has no instruction adapter", name),
			})
			continue
		}

		status, err := syncTarget(t, content, projectDir, dryRun, stderr)
		if err != nil {
			statuses = append(statuses, TargetStatus{
				Name:    name,
				Status:  "error",
				Message: err.Error(),
			})
			continue
		}
		statuses = append(statuses, status)
	}

	// Render and install skill targets in-process. No subprocess: the
	// absorbed internal/skills pipeline runs here, so a released symbrain
	// works without the archived symskills binary. Per-target failures are
	// reported as Result entries with Status "error".
	skillsResults, skillsErr := skillsrunner.Run(context.Background(), harnessNames, skillsrunner.DefaultOptions(), dryRun)
	if skillsErr != nil {
		if TargetsFailed(statuses) {
			return statuses, skillsResults, errors.Join(
				fmt.Errorf("sync: one or more instruction targets failed"),
				fmt.Errorf("sync: skills: %w", skillsErr),
			)
		}
		return statuses, skillsResults, fmt.Errorf("sync: skills: %w", skillsErr)
	}
	if TargetsFailed(statuses) {
		// All target statuses, including successes and skips, are returned so
		// callers can serialize/report the complete partial-sync result before
		// acting on this error.
		return statuses, skillsResults, errors.New("sync: one or more instruction targets failed")
	}

	return statuses, skillsResults, nil
}

func syncTarget(t adapter.Target, content, projectDir string, dryRun bool, stderr io.Writer) (TargetStatus, error) {
	relativeTarget := t.Filename
	if t.Dir != "" {
		relativeTarget = filepath.Join(t.Dir, t.Filename)
	}
	path := filepath.Join(projectDir, relativeTarget)

	file, err := instructions.OpenAtomicFile(projectDir, relativeTarget, false)
	if err != nil && !os.IsNotExist(err) {
		return TargetStatus{}, fmt.Errorf("open %s: %w", path, err)
	}
	existed := false
	var existingBytes []byte
	if err == nil {
		existingBytes, err = file.ReadBounded()
		existed = err == nil
		if err != nil && !os.IsNotExist(err) {
			_ = file.Close()
			return TargetStatus{}, fmt.Errorf("read %s: %w", path, err)
		}
	}
	if file != nil {
		defer file.Close()
	}
	existing := string(existingBytes)

	rendered := t.Render(existing, content, projectDir)
	if rendered == existing {
		return TargetStatus{
			Name:   t.Name,
			Path:   path,
			Status: "unchanged",
		}, nil
	}

	if dryRun {
		return TargetStatus{
			Name:    t.Name,
			Path:    path,
			Status:  "dry-run",
			Message: "would update",
		}, nil
	}

	if file == nil {
		file, err = instructions.OpenAtomicFile(projectDir, relativeTarget, true)
		if err != nil {
			return TargetStatus{}, fmt.Errorf("open %s for write: %w", path, err)
		}
		defer file.Close()
	}
	if err := file.Write([]byte(rendered)); err != nil {
		return TargetStatus{}, fmt.Errorf("write %s: %w", path, err)
	}

	status := "updated"
	if !existed {
		status = "created"
	}

	return TargetStatus{
		Name:   t.Name,
		Path:   path,
		Status: status,
	}, nil
}

// FormatSummary prints a human-readable summary of the sync results.
func FormatSummary(w io.Writer, statuses []TargetStatus, skillsResults []skillsrunner.Result) {
	fmt.Fprintln(w, "Instruction targets:")
	for _, s := range statuses {
		line := fmt.Sprintf("  %-12s %s", s.Name+":", s.Status)
		if s.Message != "" {
			line += " (" + s.Message + ")"
		}
		fmt.Fprintln(w, line)
	}

	if len(skillsResults) > 0 {
		fmt.Fprintln(w, "\nSkills:")
		for _, r := range skillsResults {
			line := fmt.Sprintf("  %-12s %s", r.Target+":", r.Status)
			if r.Message != "" {
				line += " (" + r.Message + ")"
			}
			fmt.Fprintln(w, line)
		}
	}
}

// Summary is the stable JSON payload returned by the sync command.
type Summary struct {
	Targets []TargetStatus        `json:"targets"`
	Skills  []skillsrunner.Result `json:"skills"`
}

// FormatSummaryJSON outputs the sync results as JSON through the shared
// renderer. It remains as a compatibility helper for package-local callers.
func FormatSummaryJSON(w io.Writer, statuses []TargetStatus, skillsResults []skillsrunner.Result) error {
	return output.Render(w, output.FormatJSON, Summary{Targets: statuses, Skills: skillsResults})
}

// TargetsFailed reports whether any instruction target failed to render or write.
// The caller renders all statuses before using this result as the process exit
// decision, so partial-sync diagnostics remain visible to scripts and users.
func TargetsFailed(statuses []TargetStatus) bool {
	for _, status := range statuses {
		if status.Status == "error" {
			return true
		}
	}
	return false
}

// SkillsFailed reports whether any skill result carries a failing status.
// The sync command uses it to produce a non-zero exit while still having
// rendered the failure in the human/JSON output first.
func SkillsFailed(results []skillsrunner.Result) bool {
	for _, r := range results {
		if r.Status == "error" {
			return true
		}
	}
	return false
}
