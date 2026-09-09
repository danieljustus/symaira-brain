package proposal

import (
	"strings"

	"github.com/danieljustus/symaira-brain/guard/internal/config"
)

// resolveDelete finds the rules matching the delete criteria, applying
// the package-doc ambiguity rules: zero matches is a NoMatchError, one
// match resolves, and multiple matches return an AmbiguousError with the
// candidate list.
func resolveDelete(m config.RuleMatch, rules []config.Rule) ([]config.Rule, error) {
	var matches []config.Rule
	for _, r := range rules {
		if matchCriteria(m, r.Match) {
			matches = append(matches, r)
		}
	}
	switch len(matches) {
	case 0:
		return nil, &NoMatchError{Match: m}
	case 1:
		return matches, nil
	default:
		return nil, &AmbiguousError{Candidates: matches}
	}
}

// matchCriteria reports whether a rule's match satisfies the (possibly
// partial) criteria: every non-empty scalar criterion must equal the
// rule's field, and a non-empty CommandContains list must equal the
// rule's list element-wise.
func matchCriteria(c config.RuleMatch, m config.RuleMatch) bool {
	if c.Server != "" && c.Server != m.Server {
		return false
	}
	if c.Tool != "" && c.Tool != m.Tool {
		return false
	}
	if c.Capability != "" && c.Capability != m.Capability {
		return false
	}
	if len(c.CommandContains) > 0 && !equalStrings(c.CommandContains, m.CommandContains) {
		return false
	}
	return true
}

// matchesEqual reports whether two match criteria are identical,
// including empty CommandContains lists.
func matchesEqual(a, b config.RuleMatch) bool {
	return a.Server == b.Server && a.Tool == b.Tool && a.Capability == b.Capability &&
		equalStrings(a.CommandContains, b.CommandContains)
}

// upsertRule returns a new rule list with the rule upserted: a rule
// with an identical match is replaced in place (keeping its position),
// otherwise the rule is appended.
func upsertRule(rules []config.Rule, rule config.Rule) []config.Rule {
	next := append([]config.Rule(nil), rules...)
	for i, r := range next {
		if matchesEqual(r.Match, rule.Match) {
			next[i] = rule
			return next
		}
	}
	return append(next, rule)
}

// removeRule returns a new rule list without the uniquely matched rule.
func removeRule(rules []config.Rule, target config.Rule) []config.Rule {
	next := append([]config.Rule(nil), rules...)
	for i, r := range next {
		if matchesEqual(r.Match, target.Match) {
			return append(next[:i], next[i+1:]...)
		}
	}
	return next
}

// validDecision reports whether d is one of the six policy decisions
// accepted by the config schema (mirrors config.validate).
func validDecision(d config.Decision) bool {
	switch d {
	case config.Allow, config.Ask, config.Deny, config.Redact, config.ReadOnly, config.Sandbox:
		return true
	}
	return false
}

// emptyMatch reports whether a rule match specifies no criteria at all.
func emptyMatch(m config.RuleMatch) bool {
	return m.Server == "" && m.Tool == "" && m.Capability == "" && len(m.CommandContains) == 0
}

// equalStrings compares two string slices element-wise.
func equalStrings(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

// matchString renders match criteria in compact form for error messages.
func matchString(m config.RuleMatch) string {
	var parts []string
	if m.Server != "" {
		parts = append(parts, "server="+m.Server)
	}
	if m.Tool != "" {
		parts = append(parts, "tool="+m.Tool)
	}
	if m.Capability != "" {
		parts = append(parts, "capability="+m.Capability)
	}
	if len(m.CommandContains) > 0 {
		parts = append(parts, "command_contains="+strings.Join(m.CommandContains, ","))
	}
	if len(parts) == 0 {
		return "{}"
	}
	return "{" + strings.Join(parts, " ") + "}"
}
