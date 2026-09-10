package variant

import (
	"regexp"
	"sort"
	"strings"
)

func UnscopedText(src string) string {
	lines := strings.Split(src, "\n")
	out := make([]string, len(lines))
	inOnly := false
	inFence := false
	for i, line := range lines {
		trimmed := strings.TrimSpace(line)
		switch {
		case openSetRe.MatchString(line):
			if Kind(openSetRe.FindStringSubmatch(line)[1]) == KindOnly {
				inOnly = true
			}
			continue
		case closeRe.MatchString(line):
			if Kind(closeRe.FindStringSubmatch(line)[1]) == KindOnly {
				inOnly = false
			}
			continue
		case openBlockRe.MatchString(line):
			continue
		case strings.HasPrefix(trimmed, "```") || strings.HasPrefix(trimmed, "~~~"):
			inFence = !inFence
			continue
		}
		if inOnly || inFence {
			continue
		}
		out[i] = line
	}
	return strings.Join(out, "\n")
}

// Mention is one occurrence of a harness name in unscoped text.
type Mention struct {
	Name string
	Line int
}

// FindMentions reports where unscoped text names one of the given harnesses.
// Matching is whole-word and case-insensitive, so "~/.hermes/reports" counts
// while "hermeneutics" does not.
func FindMentions(src string, names []string) []Mention {
	if len(names) == 0 {
		return nil
	}
	var mentions []Mention
	lines := strings.Split(UnscopedText(src), "\n")
	for _, name := range names {
		pattern, err := regexp.Compile(`(?i)\b` + regexp.QuoteMeta(name) + `\b`)
		if err != nil {
			continue
		}
		for i, line := range lines {
			if line == "" {
				continue
			}
			if pattern.MatchString(line) {
				mentions = append(mentions, Mention{Name: name, Line: i + 1})
				break
			}
		}
	}
	sort.Slice(mentions, func(i, j int) bool {
		if mentions[i].Line != mentions[j].Line {
			return mentions[i].Line < mentions[j].Line
		}
		return mentions[i].Name < mentions[j].Name
	})
	return mentions
}

// CheckOverrides reports overlay block overrides that do not correspond to a
// block in the source. An override may only replace text the canonical source
// already accounts for — that is what keeps an overlay a delta instead of a
// fork. overrides maps an overlay directory name to its block ids.
