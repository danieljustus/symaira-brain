// Package skill loads, validates, and imports portable Agent Skill bundles.
package skill

import (
	"fmt"
	"path/filepath"
	"slices"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/skills/variant"
)

func ValidateSkillName(name string) error {
	if strings.TrimSpace(name) == "" {
		return fmt.Errorf("skill name is required")
	}
	if len(name) > MaxNameLength {
		return fmt.Errorf("skill name %q exceeds maximum length of %d characters", name, MaxNameLength)
	}
	if !skillNamePattern.MatchString(name) {
		return fmt.Errorf("skill name %q must be a single lowercase alphanumeric-dash segment without consecutive, leading, or trailing hyphens", name)
	}
	return nil
}

// Validate returns non-fatal validation issues for a loaded bundle.
func Validate(bundle *Bundle) []Issue {
	if bundle == nil {
		return []Issue{{Code: "bundle_required", Severity: "error", Message: "bundle is nil"}}
	}
	var issues []Issue
	name := bundle.Frontmatter.Name
	if strings.TrimSpace(name) == "" {
		issues = append(issues, Issue{Code: "name_required", Severity: "error", Message: "frontmatter name is required", Path: "SKILL.md"})
	} else {
		if len(name) > MaxNameLength {
			issues = append(issues, Issue{Code: "name_too_long", Severity: "error", Message: fmt.Sprintf("name exceeds maximum length of %d characters (actual: %d)", MaxNameLength, len(name)), Path: "SKILL.md"})
		}
		if !skillNamePattern.MatchString(name) {
			issues = append(issues, Issue{Code: "name_format", Severity: "error", Message: "name must use lowercase letters, numbers, and single dashes (no consecutive, leading, or trailing dashes)", Path: "SKILL.md"})
		}
		// The agentskills specification requires the skill name to match the
		// parent directory name, which is how the library layout stores
		// bundles (library/<name>).
		if filepath.Base(bundle.Root) != name {
			issues = append(issues, Issue{Code: "name_dir_mismatch", Severity: "error", Message: fmt.Sprintf("frontmatter name %q must match the parent directory name %q", name, filepath.Base(bundle.Root)), Path: "SKILL.md"})
		}
	}
	if strings.TrimSpace(bundle.Frontmatter.Description) == "" {
		issues = append(issues, Issue{Code: "description_required", Severity: "error", Message: "frontmatter description is required", Path: "SKILL.md"})
	} else if len(bundle.Frontmatter.Description) > MaxDescriptionLength {
		issues = append(issues, Issue{Code: "description_too_long", Severity: "error", Message: fmt.Sprintf("frontmatter description exceeds maximum length of %d characters (actual: %d)", MaxDescriptionLength, len(bundle.Frontmatter.Description)), Path: "SKILL.md"})
	}
	if strings.TrimSpace(bundle.Frontmatter.Category) == "" {
		issues = append(issues, Issue{Code: "category_required", Severity: "warning", Message: "frontmatter category is required; reuse an existing category when possible", Path: "SKILL.md"})
	}
	if strings.TrimSpace(bundle.Body) == "" {
		issues = append(issues, Issue{Code: "body_required", Severity: "error", Message: "SKILL.md body is empty", Path: "SKILL.md"})
	} else if len(bundle.Body) > MaxBodyLength {
		issues = append(issues, Issue{Code: "body_too_long", Severity: "warning", Message: fmt.Sprintf("SKILL.md body exceeds maximum length of %d characters (actual: %d)", MaxBodyLength, len(bundle.Body)), Path: "SKILL.md"})
	}
	for _, res := range bundle.Resources {
		if res.Executable {
			issues = append(issues, Issue{Code: "resource_executable", Severity: "warning", Message: "resource file is executable; install strips the executable bit unless --allow-executable (or the manifest setting) is set", Path: res.Path})
		}
		if res.Size > MaxResourceSize {
			issues = append(issues, Issue{Code: "resource_too_large", Severity: "error", Message: fmt.Sprintf("resource exceeds maximum size of %d bytes (actual: %d)", MaxResourceSize, res.Size), Path: res.Path})
		}
	}
	targets := make([]string, 0, len(bundle.Manifest.Targets))
	for target := range bundle.Manifest.Targets {
		targets = append(targets, target)
	}
	slices.Sort(targets)
	for _, target := range targets {
		cfg := bundle.Manifest.Targets[target]
		if !cfg.Enabled {
			continue
		}
		for _, rel := range []string{cfg.Prepend, cfg.Append} {
			if rel == "" {
				continue
			}
			if err := safeRelativeFile(bundle, rel); err != nil {
				issues = append(issues, Issue{Code: "overlay_reference_missing", Severity: "error", Message: err.Error(), Path: "symskills.toml:" + target})
			}
		}
	}
	issues = append(issues, validateVariants(bundle)...)
	return issues
}

// renderBlockingCodes lists the validation codes that make a render refuse
// rather than emit output. They share one property: the rendered result
// would silently misrepresent the source — a traversing overlay reference, a
// malformed marker that would ship as literal HTML, an override addressing a
// block that does not exist, or a placeholder that cannot resolve. Every
// other error-severity code is reported by `validate` and still renders.
var renderBlockingCodes = map[string]bool{
	"overlay_reference_missing":     true,
	variant.CodeMarkerMalformed:     true,
	variant.CodeBlockIDInvalid:      true,
	variant.CodeBlockNested:         true,
	variant.CodeBlockUnclosed:       true,
	variant.CodeBlockUnmatchedClose: true,
	variant.CodeBlockCloseMismatch:  true,
	variant.CodeBlockDuplicateID:    true,
	variant.CodeTargetListEmpty:     true,
	variant.CodeOverrideUnknown:     true,
	variant.CodeTermUnknown:         true,
	variant.CodeTermNameInvalid:     true,
	variant.CodeTermDefaultRequired: true,
}

// IsRenderBlocking reports whether an error-severity validation code must
// stop a render instead of only being reported.
func IsRenderBlocking(code string) bool { return renderBlockingCodes[code] }

// variantIssues converts variant problems into validation issues for one
// path. lineOffset shifts a problem's line to its real position in the file,
// which is non-zero for SKILL.md, whose body starts below the frontmatter.
func variantIssues(path string, lineOffset int, problems []variant.Problem) []Issue {
	issues := make([]Issue, 0, len(problems))
	for _, problem := range problems {
		message := problem.Message
		if problem.Line > 0 {
			message = fmt.Sprintf("line %d: %s", problem.Line+lineOffset, problem.Message)
		}
		issues = append(issues, Issue{Code: problem.Code, Severity: problem.Severity, Message: message, Path: path})
	}
	return issues
}

// validateVariants checks the harness-variant constructs across SKILL.md and
// every markdown reference: marker structure, bundle-wide unique block ids,
// resolvable terms, and overlay overrides that address a block the canonical
// source actually defines. The last check is what keeps an overlay a delta
// keyed to the source instead of a second, drifting copy of it.
func validateVariants(bundle *Bundle) []Issue {
	var issues []Issue
	known := knownTargets()

	paths := make([]string, 0, len(bundle.Markdown)+1)
	for path := range bundle.Markdown {
		paths = append(paths, path)
	}
	slices.Sort(paths)
	paths = append([]string{"SKILL.md"}, paths...)

	// Coupling is only meaningful for a skill that targets more than one
	// harness. A skill deliberately scoped to a single enabled target may
	// name it freely.
	checkCoupling := len(known) > 0 && enabledTargetCount(bundle) != 1

	var sourceIDs []string
	definedIn := map[string]string{}
	for _, path := range paths {
		text := bundle.Body
		lineOffset := bundle.BodyLineOffset
		if path != "SKILL.md" {
			text = bundle.Markdown[path]
			lineOffset = 0
		}
		if checkCoupling {
			for _, mention := range variant.FindMentions(text, known) {
				issues = append(issues, Issue{
					Code:     variant.CodeHarnessCoupling,
					Severity: variant.SeverityWarning,
					Message: fmt.Sprintf(
						"line %d: names harness %q outside any symskills:only region; every other target renders this text too — scope it with a region, move the differing part into a {{term:...}}, or disable the other targets in symskills.toml",
						mention.Line+lineOffset, mention.Name),
					Path: path,
				})
			}
		}
		scan, problems := variant.ScanText(text)
		issues = append(issues, variantIssues(path, lineOffset, problems)...)
		issues = append(issues, variantIssues(path, lineOffset, variant.CheckRegionTargets(scan.Regions, known))...)
		for _, id := range scan.BlockIDs {
			if prev, dup := definedIn[id]; dup {
				issues = append(issues, Issue{
					Code:     variant.CodeBlockDuplicateID,
					Severity: variant.SeverityError,
					Message:  fmt.Sprintf("block id %q is already defined in %s; ids address a block across the whole skill and must be unique", id, prev),
					Path:     path,
				})
				continue
			}
			definedIn[id] = path
			sourceIDs = append(sourceIDs, id)
		}
		for _, name := range scan.Terms {
			if _, ok := bundle.Manifest.Terms[name]; !ok {
				issues = append(issues, Issue{
					Code:     variant.CodeTermUnknown,
					Severity: variant.SeverityError,
					Message:  fmt.Sprintf("term %q is referenced but not defined in [terms]", name),
					Path:     path,
				})
			}
		}
	}

	overrideIDs := map[string][]string{}
	dirs := make([]string, 0, len(bundle.BlockOverrides))
	for dir := range bundle.BlockOverrides {
		dirs = append(dirs, dir)
	}
	slices.Sort(dirs)
	for _, dir := range dirs {
		for id := range bundle.BlockOverrides[dir] {
			overrideIDs[dir] = append(overrideIDs[dir], id)
		}
		if cfg, ok := bundle.Manifest.Targets[dir]; ok && !cfg.Enabled {
			issues = append(issues, Issue{
				Code:     variant.CodeOverrideUnused,
				Severity: variant.SeverityWarning,
				Message:  fmt.Sprintf("target %q is disabled but ships block overrides; they are never rendered", dir),
				Path:     "overlays/" + dir + "/" + variant.BlocksDir,
			})
		}
	}
	issues = append(issues, variantIssues("overlays", 0, variant.CheckOverrides(sourceIDs, overrideIDs))...)
	issues = append(issues, variantIssues("symskills.toml", 0, variant.CheckTerms(bundle.Manifest.Terms, known))...)
	return issues
}

// enabledTargetCount counts the targets a manifest explicitly enables. A
// skill with no manifest returns 0, which renders for every target.
func enabledTargetCount(bundle *Bundle) int {
	count := 0
	for _, cfg := range bundle.Manifest.Targets {
		if cfg.Enabled {
			count++
		}
	}
	return count
}

func safeRelativeFile(bundle *Bundle, rel string) error {
	if filepath.IsAbs(rel) {
		return fmt.Errorf("overlay reference %q must be relative", rel)
	}
	clean := filepath.Clean(rel)
	if strings.HasPrefix(clean, ".."+string(filepath.Separator)) || clean == ".." {
		return fmt.Errorf("overlay reference %q escapes skill root", rel)
	}
	if _, err := ReadBundleBytes(bundle, filepath.ToSlash(clean), MaxInputSize); err != nil {
		return fmt.Errorf("overlay reference %q: %w", rel, err)
	}
	return nil
}
