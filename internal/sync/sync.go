// Package sync orchestrates the symbrain sync command: it renders
// instruction targets for configured harnesses and triggers symskills
// orchestration.
package sync

import (
	"encoding/json"
	"fmt"
	"io"
	"os"

	"github.com/danieljustus/symaira-brain/internal/adapter"
	"github.com/danieljustus/symaira-brain/internal/harness"
	"github.com/danieljustus/symaira-brain/internal/instructions"
	"github.com/danieljustus/symaira-brain/internal/skills"
)

// TargetStatus is the outcome of syncing one adapter target.
type TargetStatus struct {
	Name    string `json:"name"`
	Path    string `json:"path"`
	Status  string `json:"status"` // created, updated, unchanged, skipped
	Message string `json:"message,omitempty"`
}

// Run executes the sync operation.  It renders instruction targets for the
// specified harnesses, then runs symskills orchestration.  When dryRun is
// true no files are written.  Returns the per-target summary.
func Run(projectDir string, harnessNames []string, dryRun bool, stderr io.Writer) ([]TargetStatus, []skills.Result, error) {
	if len(harnessNames) == 0 {
		for _, h := range harness.All {
			harnessNames = append(harnessNames, string(h.Name))
		}
	}

	// Resolve instruction source.
	source := instructions.NewSource(projectDir)
	content, err := source.Content()
	if err != nil {
		return nil, nil, fmt.Errorf("sync: load instructions: %w", err)
	}

	// Map harness names to adapters (skip harnesses without adapters).
	supported := map[string]adapter.Target{
		"agents": adapter.AgentsTarget,
		"claude": adapter.ClaudeTarget,
		"cursor": adapter.CursorTarget,
		"gemini": adapter.GeminiTarget,
	}

	var statuses []TargetStatus
	for _, name := range harnessNames {
		t, ok := supported[name]
		if !ok {
			statuses = append(statuses, TargetStatus{
				Name:    name,
				Status:  "skipped",
				Message: fmt.Sprintf("no instruction adapter for harness %q", name),
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

	// Run symskills orchestration.
	runner := skills.DefaultRunner()
	skillsResults, _ := runner.Sync(harnessNames, 30_000_000_000) // 30s

	return statuses, skillsResults, nil
}

func syncTarget(t adapter.Target, content, projectDir string, dryRun bool, stderr io.Writer) (TargetStatus, error) {
	dir := projectDir
	if t.Dir != "" {
		dir = fmt.Sprintf("%s/%s", projectDir, t.Dir)
	}
	path := fmt.Sprintf("%s/%s", dir, t.Filename)

	existed := fileExists(path)

	var existing string
	if existed {
		data, err := os.ReadFile(path)
		if err != nil {
			return TargetStatus{}, fmt.Errorf("read %s: %w", path, err)
		}
		existing = string(data)
	}

	rendered := t.Render(content, projectDir)
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

	if err := os.MkdirAll(dir, 0o700); err != nil {
		return TargetStatus{}, fmt.Errorf("mkdir %s: %w", dir, err)
	}
	if err := os.WriteFile(path, []byte(rendered), 0o600); err != nil {
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

func fileExists(path string) bool {
	_, err := os.Stat(path)
	return err == nil
}

// FormatSummary prints a human-readable summary of the sync results.
func FormatSummary(w io.Writer, statuses []TargetStatus, skillsResults []skills.Result) {
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

// FormatSummaryJSON outputs the sync results as JSON.
func FormatSummaryJSON(w io.Writer, statuses []TargetStatus, skillsResults []skills.Result) error {
	type jsonOutput struct {
		Targets []TargetStatus  `json:"targets"`
		Skills  []skills.Result `json:"skills"`
	}
	out := jsonOutput{
		Targets: statuses,
		Skills:  skillsResults,
	}

	data, err := json.Marshal(out)
	if err != nil {
		return err
	}
	fmt.Fprintln(w, string(data))
	return nil
}
