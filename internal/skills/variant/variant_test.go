package variant

import (
	"strings"
	"testing"
)

func problemCodes(problems []Problem) []string {
	codes := make([]string, 0, len(problems))
	for _, problem := range problems {
		codes = append(codes, problem.Code)
	}
	return codes
}

func hasCode(problems []Problem, code string) bool {
	for _, problem := range problems {
		if problem.Code == code {
			return true
		}
	}
	return false
}

// TestApplyWithoutMarkersIsIdentity is the no-op guarantee: text that uses no
// blocks and no terms must survive a resolution pass byte for byte, otherwise
// every existing skill would re-render on upgrade.
func TestApplyWithoutMarkersIsIdentity(t *testing.T) {
	src := "# Title\n\nA body with an <!-- ordinary --> comment and {{not_a_term}}.\n"
	result, problems := Apply(src, Options{Target: "claude"})
	if len(problems) != 0 {
		t.Fatalf("expected no problems, got %v", problemCodes(problems))
	}
	if result.Text != src {
		t.Errorf("expected identity, got %q", result.Text)
	}
	if result.Changed() {
		t.Errorf("expected Changed()==false, got blocks=%v terms=%v", result.Blocks, result.Terms)
	}
}

func TestApplyBlockUsesCanonicalTextWithoutOverride(t *testing.T) {
	src := "before\n<!-- symskills:block worker -->\ncanonical worker text\n<!-- /symskills:block -->\nafter\n"
	result, problems := Apply(src, Options{Target: "claude"})
	if len(problems) != 0 {
		t.Fatalf("unexpected problems: %v", problemCodes(problems))
	}
	want := "before\ncanonical worker text\nafter\n"
	if result.Text != want {
		t.Errorf("got %q, want %q", result.Text, want)
	}
	if len(result.Blocks) != 0 {
		t.Errorf("expected no substituted blocks, got %v", result.Blocks)
	}
	if strings.Contains(result.Text, "symskills:block") {
		t.Error("markers must never survive into rendered output")
	}
}

func TestApplyBlockOverrideReplacesRegion(t *testing.T) {
	src := "before\n<!-- symskills:block worker -->\nHermes only. Use delegate_task.\n<!-- /symskills:block -->\nafter\n"
	result, problems := Apply(src, Options{
		Target:    "claude",
		Overrides: map[string]string{"worker": "Dispatch each worker with the Agent tool.\n"},
	})
	if len(problems) != 0 {
		t.Fatalf("unexpected problems: %v", problemCodes(problems))
	}
	want := "before\nDispatch each worker with the Agent tool.\nafter\n"
	if result.Text != want {
		t.Errorf("got %q, want %q", result.Text, want)
	}
	if len(result.Blocks) != 1 || result.Blocks[0] != "worker" {
		t.Errorf("expected blocks=[worker], got %v", result.Blocks)
	}
	if strings.Contains(result.Text, "delegate_task") {
		t.Error("canonical text must be gone once a target overrides the block")
	}
}

// TestApplyEmptyOverrideDropsRegion covers the documented way to say "this
// section does not apply to this harness" without inventing replacement prose.
func TestApplyEmptyOverrideDropsRegion(t *testing.T) {
	src := "keep\n<!-- symskills:block extra -->\nremove me\n<!-- /symskills:block -->\ntail\n"
	result, _ := Apply(src, Options{Target: "codex", Overrides: map[string]string{"extra": "  \n"}})
	if result.Text != "keep\ntail\n" {
		t.Errorf("got %q, want %q", result.Text, "keep\ntail\n")
	}
	if len(result.Blocks) != 1 {
		t.Errorf("a dropped region still counts as a substitution, got %v", result.Blocks)
	}
}

func TestApplyOnlyAndExcept(t *testing.T) {
	src := "head\n" +
		"<!-- symskills:only hermes, codex -->\nhermes and codex text\n<!-- /symskills:only -->\n" +
		"<!-- symskills:except hermes -->\neveryone but hermes\n<!-- /symskills:except -->\n" +
		"tail\n"

	hermes, problems := Apply(src, Options{Target: "hermes"})
	if len(problems) != 0 {
		t.Fatalf("unexpected problems: %v", problemCodes(problems))
	}
	if hermes.Text != "head\nhermes and codex text\ntail\n" {
		t.Errorf("hermes: got %q", hermes.Text)
	}

	claude, _ := Apply(src, Options{Target: "claude"})
	if claude.Text != "head\neveryone but hermes\ntail\n" {
		t.Errorf("claude: got %q", claude.Text)
	}
}

func TestApplyTermsPreferTargetOverDefault(t *testing.T) {
	src := "Persist the report under {{term:report_dir}}/sweep and {{term:report_dir}}/audit.\n"
	terms := map[string]map[string]string{
		"report_dir": {DefaultKey: "~/.local/state/symskills/reports", "hermes": "~/.hermes/reports"},
	}

	hermes, problems := Apply(src, Options{Target: "hermes", Terms: terms})
	if len(problems) != 0 {
		t.Fatalf("unexpected problems: %v", problemCodes(problems))
	}
	if !strings.Contains(hermes.Text, "~/.hermes/reports/sweep") || strings.Contains(hermes.Text, "{{term:") {
		t.Errorf("hermes: got %q", hermes.Text)
	}
	if hermes.Terms["report_dir"] != "~/.hermes/reports" {
		t.Errorf("expected the resolved value reported, got %v", hermes.Terms)
	}

	claude, _ := Apply(src, Options{Target: "claude", Terms: terms})
	if !strings.Contains(claude.Text, "~/.local/state/symskills/reports/sweep") {
		t.Errorf("claude: got %q", claude.Text)
	}
}

func TestApplyTermProblems(t *testing.T) {
	terms := map[string]map[string]string{"no_default": {"hermes": "hermes value"}}

	_, problems := Apply("uses {{term:missing}}\n", Options{Target: "claude", Terms: terms})
	if !hasCode(problems, CodeTermUnknown) {
		t.Errorf("expected %s, got %v", CodeTermUnknown, problemCodes(problems))
	}

	_, problems = Apply("uses {{term:no_default}}\n", Options{Target: "claude", Terms: terms})
	if !hasCode(problems, CodeTermDefaultRequired) {
		t.Errorf("expected %s, got %v", CodeTermDefaultRequired, problemCodes(problems))
	}

	result, problems := Apply("uses {{term:Bad Name}}\n", Options{Target: "claude", Terms: terms})
	if !hasCode(problems, CodeTermNameInvalid) {
		t.Errorf("expected %s, got %v", CodeTermNameInvalid, problemCodes(problems))
	}
	if !strings.Contains(result.Text, "{{term:Bad Name}}") {
		t.Error("an unresolvable placeholder must be left verbatim, not silently emptied")
	}
}

func TestParseStructuralProblems(t *testing.T) {
	cases := []struct {
		name string
		src  string
		code string
	}{
		{"unclosed", "<!-- symskills:block a -->\ntext\n", CodeBlockUnclosed},
		{"unmatched close", "text\n<!-- /symskills:block -->\n", CodeBlockUnmatchedClose},
		{"nested", "<!-- symskills:block a -->\n<!-- symskills:block b -->\nx\n<!-- /symskills:block -->\n<!-- /symskills:block -->\n", CodeBlockNested},
		{"duplicate id", "<!-- symskills:block a -->\nx\n<!-- /symskills:block -->\n<!-- symskills:block a -->\ny\n<!-- /symskills:block -->\n", CodeBlockDuplicateID},
		{"close mismatch", "<!-- symskills:only hermes -->\nx\n<!-- /symskills:block -->\n", CodeBlockCloseMismatch},
		{"invalid id", "<!-- symskills:block Bad_ID -->\nx\n<!-- /symskills:block -->\n", CodeBlockIDInvalid},
		{"malformed marker", "<!-- symskills:blok a -->\n", CodeMarkerMalformed},
		{"empty target list", "<!-- symskills:only , -->\nx\n<!-- /symskills:only -->\n", CodeTargetListEmpty},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			_, problems := ScanText(tc.src)
			if !hasCode(problems, tc.code) {
				t.Errorf("expected %s, got %v", tc.code, problemCodes(problems))
			}
		})
	}
}

func TestScanTextInventory(t *testing.T) {
	src := "<!-- symskills:block b-two -->\nx\n<!-- /symskills:block -->\n" +
		"<!-- symskills:block a-one -->\ny\n<!-- /symskills:block -->\n" +
		"<!-- symskills:only claude -->\nz\n<!-- /symskills:only -->\n" +
		"{{term:beta}} and {{term:alpha}} and {{term:alpha}}\n"
	scan, problems := ScanText(src)
	if len(problems) != 0 {
		t.Fatalf("unexpected problems: %v", problemCodes(problems))
	}
	if got := strings.Join(scan.BlockIDs, ","); got != "a-one,b-two" {
		t.Errorf("block ids: got %q, want sorted a-one,b-two", got)
	}
	if got := strings.Join(scan.Terms, ","); got != "alpha,beta" {
		t.Errorf("terms: got %q, want deduped sorted alpha,beta", got)
	}
	if len(scan.Regions) != 3 {
		t.Errorf("expected 3 regions, got %d", len(scan.Regions))
	}
}

// TestCheckOverridesRejectsUnknownID is the anti-drift guard: an overlay may
// only replace text the canonical source already accounts for.
func TestCheckOverridesRejectsUnknownID(t *testing.T) {
	problems := CheckOverrides([]string{"worker"}, map[string][]string{
		"claude": {"worker", "invented"},
	})
	if len(problems) != 1 || problems[0].Code != CodeOverrideUnknown {
		t.Fatalf("expected one %s, got %v", CodeOverrideUnknown, problemCodes(problems))
	}
	if !strings.Contains(problems[0].Message, "invented") {
		t.Errorf("message must name the offending block: %s", problems[0].Message)
	}
}

func TestCheckRegionTargets(t *testing.T) {
	regions := []Region{{Kind: KindOnly, Targets: []string{"claude", "hermez"}, Line: 4}}
	problems := CheckRegionTargets(regions, []string{"claude", "hermes"})
	if len(problems) != 1 || problems[0].Code != CodeTargetUnknown {
		t.Fatalf("expected %s for the typo, got %v", CodeTargetUnknown, problemCodes(problems))
	}
	if problems[0].Severity != SeverityWarning {
		t.Errorf("a typo warns, it does not block: %s", problems[0].Severity)
	}
	if len(CheckRegionTargets(regions, nil)) != 0 {
		t.Error("an empty registry must skip the check rather than guess")
	}
}

func TestCheckTerms(t *testing.T) {
	problems := CheckTerms(map[string]map[string]string{
		"good":       {DefaultKey: "value", "claude": "claude value"},
		"no_default": {"claude": "value"},
		"typo":       {DefaultKey: "value", "clod": "value"},
	}, []string{"claude", "hermes"})

	if !hasCode(problems, CodeTermDefaultRequired) {
		t.Errorf("expected %s, got %v", CodeTermDefaultRequired, problemCodes(problems))
	}
	if !hasCode(problems, CodeTargetUnknown) {
		t.Errorf("expected %s for the unknown target key, got %v", CodeTargetUnknown, problemCodes(problems))
	}
}

func TestApplyReportsDivergence(t *testing.T) {
	src := "<!-- symskills:block a -->\nshort\n<!-- /symskills:block -->\n"
	result, _ := Apply(src, Options{Target: "claude", Overrides: map[string]string{"a": "a much longer replacement"}})
	if result.SourceBytes != len(src) {
		t.Errorf("SourceBytes: got %d, want %d", result.SourceBytes, len(src))
	}
	if result.ReplacedBytes != len("a much longer replacement") {
		t.Errorf("ReplacedBytes: got %d", result.ReplacedBytes)
	}
}
