package variant

import (
	"fmt"
	"sort"
	"strings"
)

func Apply(src string, opts Options) (Result, []Problem) {
	segs, problems := parse(src)
	result := Result{SourceBytes: len(src)}

	var out []string
	for _, seg := range segs {
		if !seg.region {
			out = append(out, seg.lines...)
			continue
		}
		switch seg.kind {
		case KindBlock:
			override, ok := opts.Overrides[seg.id]
			if !ok {
				out = append(out, seg.lines...)
				continue
			}
			result.Blocks = append(result.Blocks, seg.id)
			text := strings.TrimRight(override, "\n")
			if strings.TrimSpace(text) == "" {
				// An empty override drops the region for this target.
				continue
			}
			result.ReplacedBytes += len(text)
			out = append(out, strings.Split(text, "\n")...)
		case KindOnly:
			if containsTarget(seg.targets, opts.Target) {
				out = append(out, seg.lines...)
			}
		case KindExcept:
			if !containsTarget(seg.targets, opts.Target) {
				out = append(out, seg.lines...)
			}
		}
	}
	sort.Strings(result.Blocks)

	text := strings.Join(out, "\n")
	text, termsUsed, termProblems := substituteTerms(text, opts)
	problems = append(problems, termProblems...)
	if len(termsUsed) > 0 {
		result.Terms = termsUsed
		for _, v := range termsUsed {
			result.ReplacedBytes += len(v)
		}
	}
	result.Text = text
	return result, problems
}

func containsTarget(list []string, target string) bool {
	for _, name := range list {
		if name == target {
			return true
		}
	}
	return false
}

// substituteTerms replaces every {{term:name}} placeholder. Placeholders are
// explicit tokens, so substitution is safe everywhere in the document —
// including inside fenced code blocks — without heuristics.
func substituteTerms(src string, opts Options) (string, map[string]string, []Problem) {
	if !strings.Contains(src, "{{term:") {
		return src, nil, nil
	}
	var problems []Problem
	used := map[string]string{}
	reported := map[string]bool{}
	out := termRefRe.ReplaceAllStringFunc(src, func(match string) string {
		name := strings.TrimSpace(termRefRe.FindStringSubmatch(match)[1])
		if !termNameRe.MatchString(name) {
			if !reported[match] {
				reported[match] = true
				problems = append(problems, Problem{
					Code:     CodeTermNameInvalid,
					Severity: SeverityError,
					Message:  fmt.Sprintf("term reference %q must be lowercase alphanumeric segments joined by single dashes or underscores", match),
				})
			}
			return match
		}
		values, ok := opts.Terms[name]
		if !ok {
			if !reported[name] {
				reported[name] = true
				problems = append(problems, Problem{
					Code:     CodeTermUnknown,
					Severity: SeverityError,
					Message:  fmt.Sprintf("term %q is referenced but not defined in [terms]", name),
				})
			}
			return match
		}
		if v, ok := values[opts.Target]; ok && v != "" {
			used[name] = v
			return v
		}
		v, ok := values[DefaultKey]
		if !ok || v == "" {
			if !reported[name] {
				reported[name] = true
				problems = append(problems, Problem{
					Code:     CodeTermDefaultRequired,
					Severity: SeverityError,
					Message:  fmt.Sprintf("term %q needs a %q value; it is the harness-neutral text the canonical source states", name, DefaultKey),
				})
			}
			return match
		}
		used[name] = v
		return v
	})
	if len(used) == 0 {
		return out, nil, problems
	}
	return out, used, problems
}

// UnscopedText returns src with every line no target reads as prose blanked
// out, keeping line numbers intact so a finding can point at a real line.
//
// Two things are removed:
//
//   - symskills:only regions, the construct that explicitly scopes text to
//     named harnesses. Naming a harness there is the intended use.
//   - fenced code blocks, where a harness name is usually a command being
//     demonstrated rather than an instruction being given.
//
// Block regions are deliberately kept: their canonical text is what every
// target without an override receives, so a harness-bound default is real
// coupling, not an exemption.
