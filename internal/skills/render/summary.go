package render

import (
	"sort"

	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

// Render status values reported by Summarize.
const (
	// StatusRendered: the target produces output.
	StatusRendered = "rendered"
	// StatusDisabled: the skill's manifest turns this target off.
	StatusDisabled = "disabled"
	// StatusRefused: the render is rejected — a required capability the
	// target declares it lacks, or a structural problem in the source.
	StatusRefused = "refused"
)

// VariantSummary reports, for one skill and one target, how far the rendered
// result diverges from the canonical source.
//
// Divergence is reported, never enforced. A skill whose render for one
// harness replaces most of its body is telling you it is probably two skills;
// splitting it is a human editorial decision, and this figure exists to make
// that decision visible early rather than to make it automatically.
type VariantSummary struct {
	Target Target `json:"target"`
	Status string `json:"status"`
	// Reason explains a disabled or refused status.
	Reason string `json:"reason,omitempty"`
	// Blocks lists the block ids this target overrides, sorted.
	Blocks []string `json:"blocks,omitempty"`
	// Terms maps each resolved term to the value this target received.
	Terms map[string]string `json:"terms,omitempty"`
	// Files lists the markdown resources whose content changed, sorted.
	Files []string `json:"files,omitempty"`
	// ReplacedBytes and SourceBytes are the raw inputs behind Divergence,
	// kept so a caller can aggregate without re-deriving them.
	ReplacedBytes int `json:"replaced_bytes"`
	SourceBytes   int `json:"source_bytes"`
	// Divergence is ReplacedBytes/SourceBytes in the range 0..1, or 0 when
	// the source is empty.
	Divergence float64 `json:"divergence"`
	// Warnings carries the render's non-fatal capability findings.
	Warnings []string `json:"warnings,omitempty"`
}

// Summarize renders a bundle in memory for each target and reports what each
// one diverges by. It writes nothing and never fails: a target that is
// disabled or refuses to render is reported as such, so one unrunnable target
// does not hide the picture for the others.
func Summarize(bundle *skill.Bundle, targets []Target) []VariantSummary {
	if bundle == nil {
		return nil
	}
	if len(targets) == 0 {
		targets = DefaultTargets()
	}
	summaries := make([]VariantSummary, 0, len(targets))
	for _, target := range targets {
		summary := VariantSummary{Target: target, Status: StatusRendered}
		if cfg, ok := bundle.Manifest.Targets[string(target)]; ok && !cfg.Enabled {
			summary.Status = StatusDisabled
			summary.Reason = "disabled in symskills.toml"
			summaries = append(summaries, summary)
			continue
		}
		item, err := RenderTarget(bundle, target)
		if err != nil {
			summary.Status = StatusRefused
			summary.Reason = err.Error()
			summaries = append(summaries, summary)
			continue
		}
		summary.Warnings = item.Warnings
		if report := item.Variants; report != nil {
			summary.Blocks = report.Blocks
			summary.Terms = report.Terms
			summary.Files = report.Files
			summary.ReplacedBytes = report.ReplacedBytes
			summary.SourceBytes = report.SourceBytes
			if report.SourceBytes > 0 {
				summary.Divergence = float64(report.ReplacedBytes) / float64(report.SourceBytes)
			}
		}
		summaries = append(summaries, summary)
	}
	return summaries
}

// MaxDivergence returns the largest divergence across rendered targets, which
// is the single number that answers "is this skill drifting into two skills?".
// Disabled and refused targets contribute nothing.
func MaxDivergence(summaries []VariantSummary) float64 {
	max := 0.0
	for _, summary := range summaries {
		if summary.Status == StatusRendered && summary.Divergence > max {
			max = summary.Divergence
		}
	}
	return max
}

// HasVariants reports whether any target resolves a block, a term or a
// changed file, or is prevented from rendering at all. It is the test for
// whether a variant report is worth showing: an ordinary portable skill has
// nothing to say here.
func HasVariants(summaries []VariantSummary) bool {
	for _, summary := range summaries {
		if summary.Status != StatusRendered {
			return true
		}
		if len(summary.Blocks) > 0 || len(summary.Terms) > 0 || len(summary.Files) > 0 || len(summary.Warnings) > 0 {
			return true
		}
	}
	return false
}

// SortedTermNames returns a summary's term names in a stable order, so text
// output does not reshuffle between runs.
func SortedTermNames(terms map[string]string) []string {
	names := make([]string, 0, len(terms))
	for name := range terms {
		names = append(names, name)
	}
	sort.Strings(names)
	return names
}
