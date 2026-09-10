package variant

import (
	"fmt"
	"sort"
	"strings"
)

func CheckOverrides(sourceIDs []string, overrides map[string][]string) []Problem {
	known := map[string]bool{}
	for _, id := range sourceIDs {
		known[id] = true
	}
	dirs := make([]string, 0, len(overrides))
	for dir := range overrides {
		dirs = append(dirs, dir)
	}
	sort.Strings(dirs)

	var problems []Problem
	for _, dir := range dirs {
		ids := append([]string(nil), overrides[dir]...)
		sort.Strings(ids)
		for _, id := range ids {
			if known[id] {
				continue
			}
			problems = append(problems, Problem{
				Code:     CodeOverrideUnknown,
				Severity: SeverityError,
				Message: fmt.Sprintf("overlay %s/%s/%s.md overrides block %q, which no SKILL.md or markdown reference in this skill defines",
					dir, BlocksDir, id, id),
			})
		}
	}
	return problems
}

// CheckRegionTargets reports only/except regions naming a target the binary
// does not know. A misspelled name silently drops content for every harness,
// which is the failure this check exists to prevent. An empty known list
// skips the check.
func CheckRegionTargets(regions []Region, known []string) []Problem {
	if len(known) == 0 {
		return nil
	}
	set := map[string]bool{}
	for _, name := range known {
		set[name] = true
	}
	var problems []Problem
	for _, region := range regions {
		for _, name := range region.Targets {
			if set[name] {
				continue
			}
			problems = append(problems, Problem{
				Code:     CodeTargetUnknown,
				Severity: SeverityWarning,
				Message:  fmt.Sprintf("%s region names unknown target %q; the region is kept or dropped as if that target never renders", region.Kind, name),
				Line:     region.Line,
			})
		}
	}
	return problems
}

// CheckTerms reports terms defined without the required default value, and
// term keys naming neither the default nor a known target. It complements the
// per-reference checks in Apply, which only see terms a document uses.
func CheckTerms(terms map[string]map[string]string, known []string) []Problem {
	if len(terms) == 0 {
		return nil
	}
	set := map[string]bool{}
	for _, name := range known {
		set[name] = true
	}
	names := make([]string, 0, len(terms))
	for name := range terms {
		names = append(names, name)
	}
	sort.Strings(names)

	var problems []Problem
	for _, name := range names {
		values := terms[name]
		if !termNameRe.MatchString(name) {
			problems = append(problems, Problem{
				Code:     CodeTermNameInvalid,
				Severity: SeverityError,
				Message:  fmt.Sprintf("term name %q must be lowercase alphanumeric segments joined by single dashes or underscores", name),
			})
		}
		if v, ok := values[DefaultKey]; !ok || strings.TrimSpace(v) == "" {
			problems = append(problems, Problem{
				Code:     CodeTermDefaultRequired,
				Severity: SeverityError,
				Message:  fmt.Sprintf("term %q needs a %q value; it is the harness-neutral text the canonical source states", name, DefaultKey),
			})
		}
		if len(set) == 0 {
			continue
		}
		keys := make([]string, 0, len(values))
		for key := range values {
			keys = append(keys, key)
		}
		sort.Strings(keys)
		for _, key := range keys {
			if key == DefaultKey || set[key] {
				continue
			}
			problems = append(problems, Problem{
				Code:     CodeTargetUnknown,
				Severity: SeverityWarning,
				Message:  fmt.Sprintf("term %q defines a value for unknown target %q; it will never be used", name, key),
			})
		}
	}
	return problems
}
