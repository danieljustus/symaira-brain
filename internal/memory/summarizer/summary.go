package summarizer

import (
	"strings"
)

// ExtractiveSummarizer analyzes session dialogs offline to construct a compact summary.
type ExtractiveSummarizer struct{}

// NewExtractiveSummarizer creates a new summarizer instance.
func NewExtractiveSummarizer() *ExtractiveSummarizer {
	return &ExtractiveSummarizer{}
}

type sentenceScore struct {
	text  string
	score float64
	index int
}

// SummarizeSession extracts the most informational sentences of a chat session.
func (es *ExtractiveSummarizer) SummarizeSession(sessionText string, maxSentences int) string {
	if len(strings.TrimSpace(sessionText)) == 0 {
		return ""
	}

	// Split by newline and punctuation to isolate sentence structures
	lines := strings.Split(sessionText, "\n")
	var rawSentences []string

	for _, line := range lines {
		trimmedLine := strings.TrimSpace(line)
		if len(trimmedLine) == 0 {
			continue
		}

		// Strip role labels like "User:" or "Assistant:"
		cleanLine := stripRoleLabels(trimmedLine)

		// Split by punctuation
		parts := splitSentences(cleanLine)
		for _, part := range parts {
			partTrimmed := strings.TrimSpace(part)
			// Keep sentences with meaningful length
			if len(partTrimmed) > 12 {
				rawSentences = append(rawSentences, partTrimmed)
			}
		}
	}

	if len(rawSentences) == 0 {
		return ""
	}

	// Score each sentence
	var scores []sentenceScore
	for i, s := range rawSentences {
		score := scoreSentence(s, i, len(rawSentences))
		scores = append(scores, sentenceScore{
			text:  s,
			score: score,
			index: i,
		})
	}

	// Sort by score descending to find the top K sentences
	for i := 0; i < len(scores); i++ {
		for j := i + 1; j < len(scores); j++ {
			if scores[j].score > scores[i].score {
				scores[i], scores[j] = scores[j], scores[i]
			}
		}
	}

	// Pick the top K
	limit := maxSentences
	if limit > len(scores) {
		limit = len(scores)
	}

	topScores := scores[:limit]

	// Sort back by original index to preserve chronological flow
	for i := 0; i < len(topScores); i++ {
		for j := i + 1; j < len(topScores); j++ {
			if topScores[j].index < topScores[i].index {
				topScores[i], topScores[j] = topScores[j], topScores[i]
			}
		}
	}

	// Build the summary
	var builder strings.Builder
	builder.WriteString("Session Context Summary:\n")
	seen := make(map[string]bool)

	for _, s := range topScores {
		trimmedText := strings.Trim(s.text, `*_- `)
		// Clean and capital first letter
		if len(trimmedText) > 0 {
			first := strings.ToUpper(string(trimmedText[0]))
			rest := trimmedText[1:]
			cleaned := first + rest
			if !seen[cleaned] {
				seen[cleaned] = true
				builder.WriteString("- " + cleaned + "\n")
			}
		}
	}

	return builder.String()
}

func stripRoleLabels(text string) string {
	lower := strings.ToLower(text)
	prefixes := []string{"user:", "assistant:", "agent:", "system:", "human:", "ai:", "bot:"}
	for _, p := range prefixes {
		if strings.HasPrefix(lower, p) {
			return strings.TrimSpace(text[len(p):])
		}
	}
	return text
}

func splitSentences(text string) []string {
	var sentences []string
	curr := ""
	for _, r := range text {
		if r == '.' || r == '?' || r == '!' {
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

func scoreSentence(text string, index, total int) float64 {
	score := 0.0
	lower := strings.ToLower(text)

	// Length optimization (prefer moderate size sentences)
	length := len(text)
	if length > 25 && length < 120 {
		score += 3.0
	} else if length >= 120 {
		score += 1.5 // penalty for overly verbose sentences
	}

	// High value keywords
	keywords := []string{
		"symaira", "vault", "memory", "project", "projekt", "preference",
		"bevorzuge", "like", "work", "build", "baue", "agent", "mcp", "typescript",
		"python", "golang", "sqlite", "todo", "done", "erledigt", "nächstes", "ziel",
		"goal", "roadmap", "sync", "cloud", "private", "public",
	}
	for _, kw := range keywords {
		if strings.Contains(lower, kw) {
			score += 2.0
		}
	}

	// Explicit action oriented patterns indicating final decisions or plans
	actionIndicators := []string{
		"i will", "ich werde", "let's", "wir sollten", "plan is", "plan ist",
		"resolved", "finished", "erledigt", "erstellt", "created", "setup",
	}
	for _, ind := range actionIndicators {
		if strings.Contains(lower, ind) {
			score += 4.0
		}
	}

	// Position bias: favor start (goals/context) and end (conclusions/next steps)
	positionFactor := float64(index) / float64(total)
	if positionFactor < 0.15 {
		score += 2.0 // Intro bias
	} else if positionFactor > 0.80 {
		score += 3.0 // Conclusive/Outro bias
	}

	// Negative bias for conversational padding
	paddings := []string{
		"hello", "hallo", "hi ", "thank you", "danke", "bitte", "please", "super",
		"great", "awesome", "coole", "toll", "yes", "no", "ja", "nein", "ok", "okay",
	}
	for _, pad := range paddings {
		if strings.Contains(lower, pad) {
			score -= 2.5
		}
	}

	return score
}
