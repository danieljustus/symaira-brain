package bench

import (
	"encoding/json"
	"fmt"
	"os"
	"strings"
)

// LongMemEval file layout (longmemeval_s.json): a JSON array of question objects.
// Assumptions made by this loader:
//   - Each haystack session becomes ONE memory whose content is the concatenated
//     "role: content" turns; memory IDs are "<question_id>__<session_id>" so they
//     stay unique across questions that share session IDs.
//   - Ground truth for a question is the set of memories built from its
//     answer_session_ids.
//   - Questions whose ID ends in "_abs" (the LongMemEval abstention split) or
//     that carry no answer_session_ids are treated as unanswerable: they get an
//     empty RelevantIDs set and Answerable=false, and feed the abstention metric.
//   - question_date / haystack_dates are not mapped to temporal validity; the
//     scope model is not mapped either (all memories land in "global").
type longMemEvalEntry struct {
	QuestionID         string              `json:"question_id"`
	QuestionType       string              `json:"question_type"`
	Question           string              `json:"question"`
	Answer             string              `json:"answer"`
	QuestionDate       string              `json:"question_date"`
	HaystackSessionIDs []string            `json:"haystack_session_ids"`
	HaystackDates      []string            `json:"haystack_dates"`
	HaystackSessions   [][]longMemEvalTurn `json:"haystack_sessions"`
	AnswerSessionIDs   []string            `json:"answer_session_ids"`
}

type longMemEvalTurn struct {
	Role    string `json:"role"`
	Content string `json:"content"`
}

// LoadLongMemEval reads a locally downloaded LongMemEval JSON file and maps it
// to a benchmark Corpus. The dataset is never bundled or downloaded; the caller
// supplies the path (e.g. `symmemory bench --corpus longmemeval --path <file>`).
func LoadLongMemEval(path string) (*Corpus, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("failed to read LongMemEval file: %w", err)
	}

	var entries []longMemEvalEntry
	if err := json.Unmarshal(data, &entries); err != nil {
		return nil, fmt.Errorf("failed to parse LongMemEval file %s: %w", path, err)
	}
	if len(entries) == 0 {
		return nil, fmt.Errorf("LongMemEval file %s contains no questions", path)
	}

	corpus := &Corpus{}
	for _, e := range entries {
		if e.QuestionID == "" || e.Question == "" {
			return nil, fmt.Errorf("LongMemEval entry is missing question_id or question")
		}
		if len(e.HaystackSessions) != len(e.HaystackSessionIDs) {
			return nil, fmt.Errorf("LongMemEval question %s: haystack_sessions (%d) does not match haystack_session_ids (%d)",
				e.QuestionID, len(e.HaystackSessions), len(e.HaystackSessionIDs))
		}

		memIDBySession := make(map[string]string, len(e.HaystackSessionIDs))
		for i, sessionID := range e.HaystackSessionIDs {
			memID := e.QuestionID + "__" + sessionID
			memIDBySession[sessionID] = memID
			corpus.Memories = append(corpus.Memories, FixtureMemory{
				ID:      memID,
				Content: flattenLongMemEvalSession(e.HaystackSessions[i]),
				Scope:   "global",
			})
		}

		gt := GroundTruth{
			Query:       e.Question,
			Description: fmt.Sprintf("LongMemEval %s (%s)", e.QuestionID, e.QuestionType),
			Answerable:  true,
		}
		for _, sid := range e.AnswerSessionIDs {
			if memID, ok := memIDBySession[sid]; ok {
				gt.RelevantIDs = append(gt.RelevantIDs, memID)
			}
		}
		if strings.HasSuffix(e.QuestionID, "_abs") || len(gt.RelevantIDs) == 0 {
			gt.Answerable = false
			gt.RelevantIDs = nil
		}
		corpus.Queries = append(corpus.Queries, gt)
	}

	return corpus, nil
}

func flattenLongMemEvalSession(turns []longMemEvalTurn) string {
	var b strings.Builder
	for _, t := range turns {
		if t.Content == "" {
			continue
		}
		fmt.Fprintf(&b, "%s: %s\n", t.Role, t.Content)
	}
	return strings.TrimSpace(b.String())
}

// HasUnanswerableQueries reports whether the corpus contains abstention cases.
func HasUnanswerableQueries(c *Corpus) bool {
	for _, q := range c.Queries {
		if !q.Answerable {
			return true
		}
	}
	return false
}
