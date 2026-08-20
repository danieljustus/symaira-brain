package extractor

import (
	"regexp"
	"strings"

	"github.com/danieljustus/symaira-corekit/evidencekit"
)

// Fact represents a single extracted assertion.
type Fact struct {
	Content  string                   `json:"content"`
	Category string                   `json:"category"` // preference, identity, project, general
	Metadata map[string]string        `json:"metadata"`
	Evidence []evidencekit.Extraction `json:"evidence,omitempty"` // grounded span(s) in the source text; Source is left for the caller to fill in
}

// PatternExtractor analyzes conversations and extracts key assertions.
type PatternExtractor struct {
	rules []regexRule
}

type regexRule struct {
	regex    *regexp.Regexp
	category string
	template string // Template to format the assertion, e.g., "User likes %s"
}

// NewPatternExtractor configures standard rules for English and German conversation patterns.
func NewPatternExtractor() *PatternExtractor {
	rules := []regexRule{
		// Preferences (English)
		{regexp.MustCompile(`(?i)(?:i\s+like|i\s+prefer|i\s+love)\s+([^.,;!]+)`), "preference", "User prefers %s"},
		{regexp.MustCompile(`(?i)(?:my\s+favorite)\s+([^.,;!]+)`), "preference", "User's favorite is %s"},
		// Preferences (German)
		{regexp.MustCompile(`(?i)(?:ich\s+mag|ich\s+bevorzuge|ich\s+liebe)\s+([^.,;!]+)`), "preference", "User prefers %s"},
		{regexp.MustCompile(`(?i)(?:mein\s+favorit\s+ist|mein\s+lieblings[^ ]+\s+ist)\s+([^.,;!]+)`), "preference", "User's favorite is %s"},

		// Projects & Work (English)
		{regexp.MustCompile(`(?i)(?:i\s+work\s+on|i'm\s+working\s+on|i\s+am\s+building)\s+([^.,;!]+)`), "project", "User is building %s"},
		{regexp.MustCompile(`(?i)(?:my\s+project\s+is|our\s+project\s+is)\s+([^.,;!]+)`), "project", "User's active project is %s"},
		// Projects & Work (German)
		{regexp.MustCompile(`(?i)(?:ich\s+arbeite\s+an|ich\s+baue\s+gerade|ich\s+bin\s+dabei)\s+([^.,;!]+)`), "project", "User is building %s"},
		{regexp.MustCompile(`(?i)(?:mein\s+projekt\s+ist|unsre\s+projekt\s+ist)\s+([^.,;!]+)`), "project", "User's active project is %s"},

		// Identity & Tools (English)
		{regexp.MustCompile(`(?i)(?:i\s+am\s+a|i\s+work\s+as\s+a|i'm\s+a)\s+([^.,;!]+)`), "identity", "User is a %s"},
		{regexp.MustCompile(`(?i)(?:i\s+use|i'm\s+using)\s+([^.,;!]+)`), "identity", "User uses %s"},
		// Identity & Tools (German)
		{regexp.MustCompile(`(?i)(?:ich\s+bin\s+ein|ich\s+arbeite\s+als|ich\s+bin\s+eine)\s+([^.,;!]+)`), "identity", "User is a %s"},
		{regexp.MustCompile(`(?i)(?:ich\s+nutze|ich\s+verwende)\s+([^.,;!]+)`), "identity", "User uses %s"},
	}

	return &PatternExtractor{rules: rules}
}

// ExtractFacts parses conversation text and extracts distinct facts.
func (pe *PatternExtractor) ExtractFacts(text string) []Fact {
	var facts []Fact
	seen := make(map[string]bool)

	// Clean text and split by common sentence delimiters
	sentences := splitSentences(text)

	for _, sentence := range sentences {
		trimmed := strings.TrimSpace(sentence)
		if len(trimmed) < 5 {
			continue
		}

		matched := false
		for _, rule := range pe.rules {
			loc := rule.regex.FindStringSubmatchIndex(trimmed)
			if loc != nil {
				// Extract the captured group
				match := trimmed[loc[2]:loc[3]]
				match = strings.TrimSpace(match)

				// Strip common trailing helper verbs or connectors
				match = cleanCapturedMatch(match)

				if len(match) > 1 {
					formatted := strings.Replace(rule.template, "%s", match, 1)
					if !seen[formatted] {
						seen[formatted] = true
						facts = append(facts, Fact{
							Content:  formatted,
							Category: rule.category,
							Metadata: map[string]string{
								"raw_trigger": trimmed,
								"method":      "regex_pattern",
							},
							Evidence: []evidencekit.Extraction{groundEvidence(text, trimmed, formatted)},
						})
					}
					matched = true
				}
			}
		}

		// Fallback: If no explicit regex patterns match, but the sentence contains highly critical keywords,
		// capture the sentence as a general context fact.
		if !matched && containsHighValueKeywords(trimmed) {
			cleanedFact := cleanSentenceForFact(trimmed)
			if len(cleanedFact) > 10 && !seen[cleanedFact] {
				seen[cleanedFact] = true
				facts = append(facts, Fact{
					Content:  cleanedFact,
					Category: "general",
					Metadata: map[string]string{
						"raw_trigger": trimmed,
						"method":      "keyword_filter",
					},
					Evidence: []evidencekit.Extraction{groundEvidence(text, trimmed, cleanedFact)},
				})
			}
		}
	}

	return facts
}

// groundEvidence aligns evidenceText (a sentence produced by splitSentences,
// already a substring of the original text modulo trimming) back to its
// character span in text. Source is left zero-value for the caller — the
// extractor doesn't know which session/document text came from.
func groundEvidence(text, evidenceText, factText string) evidencekit.Extraction {
	span, status := evidencekit.Align(text, evidenceText)
	return evidencekit.Extraction{
		Text:            factText,
		EvidenceText:    evidenceText,
		Span:            span,
		AlignmentStatus: status,
	}
}

func splitSentences(text string) []string {
	// Standard sentence separation splits by periods, questions, and newlines
	// We handle standard punctuation
	var sentences []string
	curr := ""
	for _, r := range text {
		if r == '.' || r == '?' || r == '!' || r == '\n' {
			if len(strings.TrimSpace(curr)) > 0 {
				sentences = append(sentences, curr)
			}
			curr = ""
		} else {
			curr += string(r)
		}
	}
	if len(strings.TrimSpace(curr)) > 0 {
		sentences = append(sentences, curr)
	}
	return sentences
}

func cleanCapturedMatch(text string) string {
	// Trim trailing periods, spaces, particles
	t := strings.TrimSpace(text)
	t = strings.Trim(t, `."';!?,`)

	// Remove leading auxiliary or filler words
	fillers := []string{"gerade", "jetzt", "momentan", "basically", "mostly", "always", "immer"}
	for _, f := range fillers {
		if strings.HasPrefix(strings.ToLower(t), f+" ") {
			t = t[len(f)+1:]
		}
	}
	return strings.TrimSpace(t)
}

func containsHighValueKeywords(text string) bool {
	keywords := []string{
		"symaira", "vault", "memory", "typescript", "python", "golang", "sqlite",
		"project", "projekt", "preference", "bevorzuge", "lieblings", "arbeit",
		"work", "build", "baue", "agent", "mcp", "config", "token", "database",
	}
	lower := strings.ToLower(text)
	for _, kw := range keywords {
		if strings.Contains(lower, kw) {
			return true
		}
	}
	return false
}

func cleanSentenceForFact(text string) string {
	t := strings.TrimSpace(text)
	t = strings.Trim(t, `*-_# `)

	// Replace "ich/I" with "User" in the final text for descriptive memory statements
	// A simple mapping
	replacer := strings.NewReplacer(
		"ich arbeite", "User arbeitet",
		"ich baue", "User baut",
		"ich bin", "User ist",
		"ich bevorzuge", "User bevorzugt",
		"ich nutze", "User nutzt",
		"ich verwende", "User verwendet",
		"i am working", "User is working",
		"i'm working", "User is working",
		"i am building", "User is building",
		"i'm building", "User is building",
		"i am a", "User is a",
		"i'm a", "User is a",
		"i like", "User likes",
		"i prefer", "User prefers",
		"i use", "User uses",
	)

	return replacer.Replace(t)
}
