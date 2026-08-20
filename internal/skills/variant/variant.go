// Package variant resolves harness-specific regions and terms inside
// portable skill text.
//
// The portable source stays the single source of truth: the canonical text
// is complete and readable on its own, and each harness's deviation is a
// named delta keyed to it. Three constructs express those deltas.
//
// A block marks a region a target may replace wholesale:
//
//	<!-- symskills:block worker-execution -->
//	Canonical text, used unless a target overrides this block.
//	<!-- /symskills:block -->
//
// The replacement lives in overlays/<target>/blocks/worker-execution.md.
// An override that is empty (or whitespace only) drops the region for that
// target.
//
// only/except keep or drop a region without a second file:
//
//	<!-- symskills:only hermes,codex -->
//	Text kept only for the listed targets.
//	<!-- /symskills:only -->
//
// A term substitutes a single phrase from the manifest's [terms] table:
//
//	Persist the report under {{term:report_dir}}/issue-sweep/.
//
// Markers never reach a rendered skill: they are stripped for every target,
// including the targets that change nothing. Regions do not nest — a flat
// structure keeps both the parser and a human diff unambiguous.
package variant

import (
	"fmt"
	"regexp"
	"sort"
	"strings"
)

// Severity levels mirror skill.Issue so callers can map problems one to one.
const (
	SeverityError   = "error"
	SeverityWarning = "warning"
)

// Problem codes reported by Scan, Apply, and the override checks.
const (
	CodeMarkerMalformed     = "variant_marker_malformed"
	CodeBlockIDInvalid      = "block_id_invalid"
	CodeBlockNested         = "block_nested"
	CodeBlockUnclosed       = "block_unclosed"
	CodeBlockUnmatchedClose = "block_unmatched_close"
	CodeBlockCloseMismatch  = "block_close_mismatch"
	CodeBlockDuplicateID    = "block_duplicate_id"
	CodeTargetListEmpty     = "block_target_list_empty"
	CodeTargetUnknown       = "block_target_unknown"
	CodeOverrideUnknown     = "block_override_unknown"
	CodeOverrideUnused      = "block_override_unused"
	CodeTermUnknown         = "term_unknown"
	CodeTermNameInvalid     = "term_name_invalid"
	CodeTermDefaultRequired = "term_default_required"
	CodeHarnessCoupling     = "harness_coupling"
)

// DefaultKey is the terms-table key holding the harness-neutral value. Every
// term must define it so the canonical source always states something true.
const DefaultKey = "default"

// BlocksDir is the overlay subdirectory holding per-target block overrides.
const BlocksDir = "blocks"

var (
	// markerProbe recognises anything that looks like a symskills marker so a
	// typo is reported instead of silently rendering as literal HTML comment.
	markerProbe = regexp.MustCompile(`<!--\s*/?\s*symskills:`)
	openBlockRe = regexp.MustCompile(`^\s*<!--\s*symskills:block\s+(\S+)\s*-->\s*$`)
	openSetRe   = regexp.MustCompile(`^\s*<!--\s*symskills:(only|except)\s+(.+?)\s*-->\s*$`)
	closeRe     = regexp.MustCompile(`^\s*<!--\s*/symskills:(block|only|except)\s*-->\s*$`)
	blockIDRe   = regexp.MustCompile(`^[a-z0-9]+(?:-[a-z0-9]+)*$`)
	termRefRe   = regexp.MustCompile(`\{\{term:([^}]*)\}\}`)
	termNameRe  = regexp.MustCompile(`^[a-z0-9]+(?:[_-][a-z0-9]+)*$`)
)

// Kind distinguishes the three region types.
type Kind string

const (
	KindBlock  Kind = "block"
	KindOnly   Kind = "only"
	KindExcept Kind = "except"
)

// Problem is one structural or reference finding. Line is 1-based; 0 means
// the finding is not tied to a specific line.
type Problem struct {
	Code     string
	Severity string
	Message  string
	Line     int
}

func (p Problem) String() string {
	if p.Line > 0 {
		return fmt.Sprintf("line %d: %s", p.Line, p.Message)
	}
	return p.Message
}

// Region describes one marked region found in a source text.
type Region struct {
	Kind    Kind
	ID      string
	Targets []string
	Line    int
}

// Scan is the structural inventory of one source text.
type Scan struct {
	Regions  []Region
	BlockIDs []string
	Terms    []string
}

// Options controls one Apply pass.
type Options struct {
	// Target is the harness target name the text is rendered for.
	Target string
	// Overrides maps a block id to its replacement text for this target.
	Overrides map[string]string
	// Terms is the manifest terms table: term name -> (DefaultKey|target) -> value.
	Terms map[string]map[string]string
}

// Result is the outcome of one Apply pass.
type Result struct {
	// Text is the resolved output with every marker stripped.
	Text string
	// Blocks lists the block ids replaced by an override, sorted.
	Blocks []string
	// Terms maps each resolved term name to the value used for this target.
	Terms map[string]string
	// ReplacedBytes counts the bytes emitted by overrides and terms; with
	// SourceBytes it gives a divergence figure per target.
	ReplacedBytes int
	SourceBytes   int
}

// Changed reports whether this target's output deviates from the canonical
// text through a block override or a term substitution.
func (r Result) Changed() bool { return len(r.Blocks) > 0 || len(r.Terms) > 0 }

// segment is either a run of literal lines or one marked region.
type segment struct {
	region  bool
	kind    Kind
	id      string
	targets []string
	line    int
	lines   []string
}

// parse splits src into literal and region segments. Structural problems are
// reported; parsing continues so one typo does not hide the rest.
func parse(src string) ([]segment, []Problem) {
	var (
		problems []Problem
		segs     []segment
		literal  segment
		open     *segment
	)
	flush := func() {
		if len(literal.lines) > 0 {
			segs = append(segs, literal)
			literal = segment{}
		}
	}
	bad := func(code, msg string, line int) {
		problems = append(problems, Problem{Code: code, Severity: SeverityError, Message: msg, Line: line})
	}

	seen := map[string]int{}
	for i, line := range strings.Split(src, "\n") {
		lineNo := i + 1

		if m := openBlockRe.FindStringSubmatch(line); m != nil {
			id := m[1]
			if !blockIDRe.MatchString(id) {
				bad(CodeBlockIDInvalid, fmt.Sprintf("block id %q must be lowercase alphanumeric segments joined by single dashes", id), lineNo)
				continue
			}
			if open != nil {
				bad(CodeBlockNested, fmt.Sprintf("block %q opens inside the %s region opened on line %d; regions must not nest", id, open.kind, open.line), lineNo)
				continue
			}
			if prev, dup := seen[id]; dup {
				bad(CodeBlockDuplicateID, fmt.Sprintf("block id %q is already used on line %d; ids must be unique per file", id, prev), lineNo)
				continue
			}
			seen[id] = lineNo
			flush()
			open = &segment{region: true, kind: KindBlock, id: id, line: lineNo}
			continue
		}

		if m := openSetRe.FindStringSubmatch(line); m != nil {
			kind := Kind(m[1])
			if open != nil {
				bad(CodeBlockNested, fmt.Sprintf("%s region opens inside the %s region opened on line %d; regions must not nest", kind, open.kind, open.line), lineNo)
				continue
			}
			targets := splitTargets(m[2])
			if len(targets) == 0 {
				bad(CodeTargetListEmpty, fmt.Sprintf("%s region lists no target names", kind), lineNo)
				continue
			}
			flush()
			open = &segment{region: true, kind: kind, targets: targets, line: lineNo}
			continue
		}

		if m := closeRe.FindStringSubmatch(line); m != nil {
			kind := Kind(m[1])
			if open == nil {
				bad(CodeBlockUnmatchedClose, fmt.Sprintf("closing %s marker without a matching opening marker", kind), lineNo)
				continue
			}
			if open.kind != kind {
				bad(CodeBlockCloseMismatch, fmt.Sprintf("closing %s marker ends the %s region opened on line %d", kind, open.kind, open.line), lineNo)
				continue
			}
			segs = append(segs, *open)
			open = nil
			continue
		}

		if markerProbe.MatchString(line) {
			bad(CodeMarkerMalformed, fmt.Sprintf("unrecognised symskills marker: %s", strings.TrimSpace(line)), lineNo)
			continue
		}

		if open != nil {
			open.lines = append(open.lines, line)
			continue
		}
		literal.lines = append(literal.lines, line)
	}

	if open != nil {
		label := string(open.kind)
		if open.id != "" {
			label = fmt.Sprintf("block %q", open.id)
		}
		bad(CodeBlockUnclosed, fmt.Sprintf("%s region opened on line %d is never closed", label, open.line), open.line)
		// Keep the buffered content so a rendered preview is not truncated.
		open.region = false
		segs = append(segs, *open)
	}
	flush()
	return segs, problems
}

func splitTargets(raw string) []string {
	var out []string
	for _, part := range strings.Split(raw, ",") {
		if name := strings.TrimSpace(part); name != "" {
			out = append(out, name)
		}
	}
	return out
}

// ScanText inventories the regions and term references in src and reports
// structural problems. It reads nothing from disk.
func ScanText(src string) (Scan, []Problem) {
	segs, problems := parse(src)
	scan := Scan{}
	for _, seg := range segs {
		if !seg.region {
			continue
		}
		scan.Regions = append(scan.Regions, Region{Kind: seg.kind, ID: seg.id, Targets: seg.targets, Line: seg.line})
		if seg.kind == KindBlock {
			scan.BlockIDs = append(scan.BlockIDs, seg.id)
		}
	}
	names, termProblems := termRefs(src)
	scan.Terms = names
	problems = append(problems, termProblems...)
	sort.Strings(scan.BlockIDs)
	return scan, problems
}

// termRefs returns the distinct, sorted term names referenced in src.
func termRefs(src string) ([]string, []Problem) {
	var problems []Problem
	seen := map[string]bool{}
	var names []string
	for _, m := range termRefRe.FindAllStringSubmatch(src, -1) {
		name := strings.TrimSpace(m[1])
		if !termNameRe.MatchString(name) {
			problems = append(problems, Problem{
				Code:     CodeTermNameInvalid,
				Severity: SeverityError,
				Message:  fmt.Sprintf("term reference %q must be lowercase alphanumeric segments joined by single dashes or underscores", m[0]),
			})
			continue
		}
		if !seen[name] {
			seen[name] = true
			names = append(names, name)
		}
	}
	sort.Strings(names)
	return names, problems
}

// Apply resolves every region and term in src for one target and returns the
// rendered text with all markers stripped. Problems are reported rather than
// returned as an error so a caller can surface them all at once; the text is
// still usable (unresolvable placeholders are left verbatim).
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
