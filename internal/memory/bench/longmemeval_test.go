package bench

import (
	"os"
	"path/filepath"
	"testing"
)

const sampleLongMemEval = `[
  {
    "question_id": "q1",
    "question_type": "single-session-user",
    "question": "What port does the backend use?",
    "answer": "8080",
    "question_date": "2024-01-01",
    "haystack_session_ids": ["s1", "s2"],
    "haystack_dates": ["2024-01-01", "2024-01-02"],
    "haystack_sessions": [
      [
        {"role": "user", "content": "What port?"},
        {"role": "assistant", "content": "The backend uses port 8080."}
      ],
      [
        {"role": "user", "content": "Tell me about fonts."}
      ]
    ],
    "answer_session_ids": ["s1"]
  },
  {
    "question_id": "q2_abs",
    "question_type": "abstention",
    "question": "What is the airspeed velocity of an unladen swallow?",
    "answer": "",
    "question_date": "2024-01-03",
    "haystack_session_ids": ["s3"],
    "haystack_dates": ["2024-01-03"],
    "haystack_sessions": [
      [
        {"role": "user", "content": "Unrelated small talk."}
      ]
    ],
    "answer_session_ids": []
  }
]`

func writeTempCorpus(t *testing.T, content string) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), "longmemeval.json")
	if err := os.WriteFile(path, []byte(content), 0o600); err != nil {
		t.Fatalf("write fixture: %v", err)
	}
	return path
}

func TestLoadLongMemEval_MapsSessionsAndGroundTruth(t *testing.T) {
	corpus, err := LoadLongMemEval(writeTempCorpus(t, sampleLongMemEval))
	if err != nil {
		t.Fatalf("LoadLongMemEval: %v", err)
	}

	if len(corpus.Memories) != 3 {
		t.Fatalf("expected 3 memories (one per session), got %d", len(corpus.Memories))
	}
	if corpus.Memories[0].ID != "q1__s1" {
		t.Errorf("memory ID must be questionID__sessionID, got %q", corpus.Memories[0].ID)
	}
	if corpus.Memories[0].Scope != "global" {
		t.Errorf("expected scope global, got %q", corpus.Memories[0].Scope)
	}
	want := "user: What port?\nassistant: The backend uses port 8080."
	if corpus.Memories[0].Content != want {
		t.Errorf("session flattening wrong:\n got %q\nwant %q", corpus.Memories[0].Content, want)
	}

	if len(corpus.Queries) != 2 {
		t.Fatalf("expected 2 queries, got %d", len(corpus.Queries))
	}
	answerable := corpus.Queries[0]
	if !answerable.Answerable {
		t.Errorf("q1 must be answerable")
	}
	if len(answerable.RelevantIDs) != 1 || answerable.RelevantIDs[0] != "q1__s1" {
		t.Errorf("answer session s1 must map to q1__s1, got %v", answerable.RelevantIDs)
	}

	unanswerable := corpus.Queries[1]
	if unanswerable.Answerable {
		t.Errorf("_abs question must be unanswerable")
	}
	if len(unanswerable.RelevantIDs) != 0 {
		t.Errorf("unanswerable query must have no relevant IDs, got %v", unanswerable.RelevantIDs)
	}

	if !HasUnanswerableQueries(corpus) {
		t.Errorf("corpus with an _abs question must report unanswerable queries")
	}
	if HasUnanswerableQueries(&Corpus{Queries: []GroundTruth{{Answerable: true}}}) {
		t.Errorf("all-answerable corpus must not report unanswerable queries")
	}
}

func TestLoadLongMemEval_Errors(t *testing.T) {
	if _, err := LoadLongMemEval(filepath.Join(t.TempDir(), "missing.json")); err == nil {
		t.Errorf("missing file must error")
	}
	if _, err := LoadLongMemEval(writeTempCorpus(t, `{not json`)); err == nil {
		t.Errorf("malformed JSON must error")
	}
	if _, err := LoadLongMemEval(writeTempCorpus(t, `[]`)); err == nil {
		t.Errorf("empty dataset must error")
	}
	mismatched := `[{"question_id":"q1","question":"x","haystack_session_ids":["s1"],"haystack_sessions":[]}]`
	if _, err := LoadLongMemEval(writeTempCorpus(t, mismatched)); err == nil {
		t.Errorf("session/session-id count mismatch must error")
	}
}
