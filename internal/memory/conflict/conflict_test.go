package conflict

import (
	"context"
	"math"
	"math/bits"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

const testSource = "conflict-test"

func helperDB(t *testing.T) *db.DB {
	t.Helper()
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/test.db"
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatalf("Open failed: %v", err)
	}
	t.Cleanup(func() { _ = database.Close() })
	return database
}

// unitBase returns the unit vector (1, 0, 0, ...) used as the shared
// candidate/query embedding.
func unitBase() []float32 {
	v := make([]float32, db.EmbeddingDim)
	v[0] = 1.0
	return v
}

// bandVector returns a unit vector with cosine ≈ target against unitBase
// whose LSH hash is within Hamming distance 2 of unitBase's hash, so
// SearchMemories (radius 2) deterministically recalls it.
func bandVector(t *testing.T, target float32) []float32 {
	t.Helper()
	base := unitBase()
	baseHash, err := db.ComputeLSH(base)
	if err != nil {
		t.Fatalf("ComputeLSH: %v", err)
	}
	orth := float32(math.Sqrt(1 - float64(target)*float64(target)))
	for j := 1; j < 64; j++ {
		v := make([]float32, db.EmbeddingDim)
		v[0] = target
		v[j] = orth
		h, err := db.ComputeLSH(v)
		if err != nil {
			t.Fatalf("ComputeLSH: %v", err)
		}
		if bits.OnesCount(uint(h^baseHash)) <= 2 {
			return v
		}
	}
	t.Fatalf("no band vector found for target %v", target)
	return nil
}

func saveTestMemory(t *testing.T, database *db.DB, id, content string, emb []float32, importance float64, createdAt time.Time) *db.Memory {
	t.Helper()
	m := &db.Memory{
		ID:                  id,
		Content:             content,
		Scope:               "global",
		Metadata:            map[string]string{},
		Embedding:           emb,
		EmbeddingSource:     testSource,
		EmbeddingModel:      "test-model",
		CreatedBy:           "actor-" + id,
		CreatedSession:      "sess-" + id,
		CreatedAt:           createdAt,
		UpdatedAt:           createdAt,
		Importance:          importance,
		ConsolidationStatus: "raw",
	}
	if err := database.SaveMemory(m); err != nil {
		t.Fatalf("SaveMemory(%s): %v", id, err)
	}
	return m
}

func newWrite(id, content string, emb []float32, importance float64) *db.Memory {
	return &db.Memory{
		ID:              id,
		Content:         content,
		Scope:           "global",
		Metadata:        map[string]string{},
		Embedding:       emb,
		EmbeddingSource: testSource,
		CreatedBy:       "writer",
		CreatedSession:  "writer-sess",
		Importance:      importance,
	}
}

func defaultCfg() config.ConflictConfig {
	return config.ConflictConfig{
		Enabled:                true,
		ContradictionThreshold: 0.80,
		NearDupThreshold:       0.95,
		MaxCandidates:          10,
	}
}

// fakeProvider returns the configured verdicts for every call; an empty
// list means "contradiction for all pairs".
type fakeProvider struct {
	verdicts []Verdict
	err      error
}

func (f *fakeProvider) Verdicts(_ context.Context, pairs []Pair) ([]Verdict, error) {
	if f.err != nil {
		return nil, f.err
	}
	if len(f.verdicts) == 0 {
		out := make([]Verdict, len(pairs))
		for i := range out {
			out[i] = VerdictContradiction
		}
		return out, nil
	}
	return f.verdicts, nil
}

func TestCheckDisabledRestoresLegacy(t *testing.T) {
	database := helperDB(t)
	now := time.Now().UTC().Add(-time.Hour)
	// A byte-identical fact already exists in the same scope.
	saveTestMemory(t, database, "old-1", "the daemon runs on port 8787", unitBase(), 0.5, now)

	cfg := defaultCfg()
	cfg.Enabled = false
	c := NewChecker(database, cfg)

	res, err := c.Check(context.Background(), newWrite("new-1", "the daemon runs on port 8787", unitBase(), 0.5))
	if err != nil {
		t.Fatalf("Check: %v", err)
	}
	if len(res) != 0 {
		t.Fatalf("disabled check must produce no resolutions, got %d", len(res))
	}
}

func TestCheckSkipsWithoutEmbeddings(t *testing.T) {
	database := helperDB(t)
	c := NewChecker(database, defaultCfg())
	m := newWrite("new-1", "no embedding here", nil, 0.5)
	res, err := c.Check(context.Background(), m)
	if err != nil {
		t.Fatalf("Check: %v", err)
	}
	if len(res) != 0 {
		t.Fatalf("expected no resolutions without embeddings, got %d", len(res))
	}
}

func TestCheckExactHashRepeat(t *testing.T) {
	database := helperDB(t)
	old := saveTestMemory(t, database, "old-1", "the daemon runs on port 8787", unitBase(), 0.5, time.Now().UTC().Add(-time.Hour))

	c := NewChecker(database, defaultCfg())
	res, err := c.Check(context.Background(), newWrite("new-1", "the daemon runs on port 8787", unitBase(), 0.5))
	if err != nil {
		t.Fatalf("Check: %v", err)
	}
	if len(res) != 1 || res[0].Verdict != VerdictRepeat {
		t.Fatalf("expected a single repeat resolution, got %+v", res)
	}
	if res[0].Candidate.ID != old.ID {
		t.Errorf("repeat must reference the existing row %s, got %s", old.ID, res[0].Candidate.ID)
	}
	if res[0].Similarity != 1.0 {
		t.Errorf("expected similarity 1.0 for byte-identical content, got %v", res[0].Similarity)
	}
}

func TestCheckExactHashRepeatSkipsSuperseded(t *testing.T) {
	database := helperDB(t)
	now := time.Now().UTC().Add(-time.Hour)
	old := saveTestMemory(t, database, "old-1", "the daemon runs on port 8787", unitBase(), 0.5, now)
	// The old fact is already resolved (superseded by a newer one).
	if err := database.SupersedeMemory(old.ID, "winner-1", now, "actor", "sess"); err != nil {
		t.Fatalf("SupersedeMemory: %v", err)
	}

	c := NewChecker(database, defaultCfg())
	res, err := c.Check(context.Background(), newWrite("new-1", "the daemon runs on port 8787", unitBase(), 0.5))
	if err != nil {
		t.Fatalf("Check: %v", err)
	}
	if len(res) != 0 {
		t.Fatalf("superseded rows must not be dedup targets, got %+v", res)
	}
}

func TestCheckNearDupReplacesOldest(t *testing.T) {
	database := helperDB(t)
	old := saveTestMemory(t, database, "old-1", "the daemon runs on port 8787", unitBase(), 0.5, time.Now().UTC().Add(-time.Hour))

	// 0.98 cosine: same fact region (≥ 0.95 near-dup threshold). Like
	// consolidation, the newer representation replaces the older row.
	c := NewChecker(database, defaultCfg())
	res, err := c.Check(context.Background(), newWrite("new-1", "the daemon runs on port 9000", bandVector(t, 0.98), 0.5))
	if err != nil {
		t.Fatalf("Check: %v", err)
	}
	if len(res) != 1 || res[0].Verdict != VerdictContradiction {
		t.Fatalf("expected a single near-dup replacement, got %+v", res)
	}
	if res[0].WinnerID != "new-1" || res[0].LoserID != old.ID {
		t.Errorf("near-dup must replace the older row: winner=%s loser=%s", res[0].WinnerID, res[0].LoserID)
	}
	if res[0].Rule != "near_dup" {
		t.Errorf("expected rule near_dup, got %q", res[0].Rule)
	}
}

func TestCheckNearDupShortCircuitsBand(t *testing.T) {
	database := helperDB(t)
	now := time.Now().UTC().Add(-time.Hour)
	saveTestMemory(t, database, "near-1", "near duplicate fact", bandVector(t, 0.98), 0.5, now.Add(-2*time.Hour))
	saveTestMemory(t, database, "band-1", "band fact", bandVector(t, 0.85), 0.5, now)

	c := NewChecker(database, defaultCfg())
	res, err := c.Check(context.Background(), newWrite("new-1", "some content", unitBase(), 0.5))
	if err != nil {
		t.Fatalf("Check: %v", err)
	}
	if len(res) != 1 || res[0].Verdict != VerdictContradiction {
		t.Fatalf("near-dup must short-circuit the band, got %+v", res)
	}
	if res[0].LoserID != "near-1" {
		t.Errorf("expected replacement of near-1, got loser=%s", res[0].LoserID)
	}
}

func TestCheckBandAmbiguousByDefault(t *testing.T) {
	database := helperDB(t)
	cand := saveTestMemory(t, database, "band-1", "the daemon listens on port 8787", bandVector(t, 0.90), 0.5, time.Now().UTC().Add(-time.Hour))

	c := NewChecker(database, defaultCfg())
	res, err := c.Check(context.Background(), newWrite("new-1", "the daemon runs on port 9000", unitBase(), 0.5))
	if err != nil {
		t.Fatalf("Check: %v", err)
	}
	if len(res) != 1 {
		t.Fatalf("expected one resolution, got %+v", res)
	}
	if res[0].Verdict != VerdictAmbiguous {
		t.Fatalf("default verdict tier must be ambiguous, got %v", res[0].Verdict)
	}
	if res[0].Candidate.ID != cand.ID {
		t.Errorf("expected candidate %s, got %s", cand.ID, res[0].Candidate.ID)
	}
}

func TestCheckBandContradictionResolvesNewWins(t *testing.T) {
	database := helperDB(t)
	now := time.Now().UTC().Add(-time.Hour)
	cand := saveTestMemory(t, database, "band-1", "the daemon listens on port 8787", bandVector(t, 0.90), 0.5, now)

	c := NewChecker(database, defaultCfg())
	c.SetVerdictProvider(&fakeProvider{}) // contradiction for all pairs
	res, err := c.Check(context.Background(), newWrite("new-1", "the daemon runs on port 9000", unitBase(), 0.5))
	if err != nil {
		t.Fatalf("Check: %v", err)
	}
	if len(res) != 1 || res[0].Verdict != VerdictContradiction {
		t.Fatalf("expected a contradiction resolution, got %+v", res)
	}
	if res[0].WinnerID != "new-1" || res[0].LoserID != cand.ID {
		t.Errorf("newer write must win by recency: winner=%s loser=%s", res[0].WinnerID, res[0].LoserID)
	}
	if res[0].Rule != "recency" {
		t.Errorf("expected rule recency, got %q", res[0].Rule)
	}
	if res[0].WinnerActor != "writer" || res[0].LoserActor != cand.CreatedBy {
		t.Errorf("actors not recorded: winner=%q loser=%q", res[0].WinnerActor, res[0].LoserActor)
	}
}

func TestCheckBandContradictionImportanceWins(t *testing.T) {
	database := helperDB(t)
	now := time.Now().UTC().Add(-time.Hour)
	// The curated old fact outranks the flaky new write.
	cand := saveTestMemory(t, database, "band-1", "the daemon listens on port 8787", bandVector(t, 0.90), 0.9, now)

	c := NewChecker(database, defaultCfg())
	c.SetVerdictProvider(&fakeProvider{})
	res, err := c.Check(context.Background(), newWrite("new-1", "the daemon runs on port 9000", unitBase(), 0.3))
	if err != nil {
		t.Fatalf("Check: %v", err)
	}
	if len(res) != 1 || res[0].Verdict != VerdictContradiction {
		t.Fatalf("expected a contradiction resolution, got %+v", res)
	}
	if res[0].WinnerID != cand.ID || res[0].LoserID != "new-1" {
		t.Errorf("higher importance must win: winner=%s loser=%s", res[0].WinnerID, res[0].LoserID)
	}
	if res[0].Rule != "importance" {
		t.Errorf("expected rule importance, got %q", res[0].Rule)
	}
}

func TestCheckBandLLMRepeatReplacesOldest(t *testing.T) {
	database := helperDB(t)
	cand := saveTestMemory(t, database, "band-1", "alice prefers dark mode in all applications", bandVector(t, 0.90), 0.5, time.Now().UTC().Add(-time.Hour))

	c := NewChecker(database, defaultCfg())
	c.SetVerdictProvider(&fakeProvider{verdicts: []Verdict{VerdictRepeat}})
	res, err := c.Check(context.Background(), newWrite("new-1", "alice prefers dark mode", unitBase(), 0.5))
	if err != nil {
		t.Fatalf("Check: %v", err)
	}
	if len(res) != 1 || res[0].Verdict != VerdictContradiction {
		t.Fatalf("LLM repeat verdict must replace the older row, got %+v", res)
	}
	if res[0].WinnerID != "new-1" || res[0].LoserID != cand.ID {
		t.Errorf("expected new-1 to replace %s, got winner=%s loser=%s", cand.ID, res[0].WinnerID, res[0].LoserID)
	}
	if res[0].Rule != "repeat" {
		t.Errorf("expected rule repeat, got %q", res[0].Rule)
	}
}

// contentFakeProvider returns a verdict per pair based on the candidate's
// content: pairs whose old content mentions a port are contradictions,
// everything else is a repeat. Order-independent, so tests stay robust
// against candidate ranking.
type contentFakeProvider struct{}

func (contentFakeProvider) Verdicts(_ context.Context, pairs []Pair) ([]Verdict, error) {
	out := make([]Verdict, len(pairs))
	for i, p := range pairs {
		if strings.Contains(p.OldContent, "port") {
			out[i] = VerdictContradiction
		} else {
			out[i] = VerdictRepeat
		}
	}
	return out, nil
}

func TestCheckBandLLMContradictionAndRepeatBothResolve(t *testing.T) {
	database := helperDB(t)
	now := time.Now().UTC().Add(-time.Hour)
	saveTestMemory(t, database, "band-1", "alice prefers dark mode in all applications", bandVector(t, 0.88), 0.5, now.Add(-2*time.Hour))
	saveTestMemory(t, database, "band-2", "the daemon listens on port 8787", bandVector(t, 0.90), 0.5, now)

	c := NewChecker(database, defaultCfg())
	c.SetVerdictProvider(contentFakeProvider{})
	res, err := c.Check(context.Background(), newWrite("new-1", "the daemon runs on port 9000", unitBase(), 0.5))
	if err != nil {
		t.Fatalf("Check: %v", err)
	}
	// Both band pairs resolve in favor of the new content: the repeat
	// (band-1) and the contradiction (band-2) both get superseded.
	if len(res) != 2 {
		t.Fatalf("expected two resolutions, got %+v", res)
	}
	losers := map[string]string{}
	for _, r := range res {
		if r.Verdict != VerdictContradiction || r.WinnerID != "new-1" {
			t.Fatalf("all band verdicts must resolve with the new content winning, got %+v", r)
		}
		losers[r.LoserID] = r.Rule
	}
	if losers["band-1"] != "repeat" {
		t.Errorf("band-1 must resolve as a repeat, got %q", losers["band-1"])
	}
	if losers["band-2"] != "recency" {
		t.Errorf("band-2 must resolve by recency, got %q", losers["band-2"])
	}
}

func TestCheckBandProviderErrorDegradesToAmbiguous(t *testing.T) {
	database := helperDB(t)
	cand := saveTestMemory(t, database, "band-1", "the daemon listens on port 8787", bandVector(t, 0.90), 0.5, time.Now().UTC().Add(-time.Hour))

	c := NewChecker(database, defaultCfg())
	c.SetVerdictProvider(&fakeProvider{err: context.DeadlineExceeded})
	res, err := c.Check(context.Background(), newWrite("new-1", "the daemon runs on port 9000", unitBase(), 0.5))
	if err != nil {
		t.Fatalf("Check must not fail on provider error: %v", err)
	}
	if len(res) != 1 || res[0].Verdict != VerdictAmbiguous {
		t.Fatalf("provider error must degrade to ambiguous, got %+v", res)
	}
	if res[0].Candidate.ID != cand.ID {
		t.Errorf("expected candidate %s, got %s", cand.ID, res[0].Candidate.ID)
	}
}

func TestCheckIgnoresUnrelatedCandidates(t *testing.T) {
	database := helperDB(t)
	saveTestMemory(t, database, "low-1", "unrelated fact", bandVector(t, 0.50), 0.5, time.Now().UTC().Add(-time.Hour))

	c := NewChecker(database, defaultCfg())
	res, err := c.Check(context.Background(), newWrite("new-1", "some content", unitBase(), 0.5))
	if err != nil {
		t.Fatalf("Check: %v", err)
	}
	if len(res) != 0 {
		t.Fatalf("below-threshold candidates must produce no resolutions, got %+v", res)
	}
}

func TestCheckOtherScopeIgnored(t *testing.T) {
	database := helperDB(t)
	saveTestMemory(t, database, "other-1", "the daemon listens on port 8787", unitBase(), 0.5, time.Now().UTC().Add(-time.Hour))
	// Same content hash, different scope: cross-scope facts are not
	// conflicts and are not dedup targets.
	m := newWrite("new-1", "the daemon listens on port 8787", unitBase(), 0.5)
	m.Scope = "project"

	c := NewChecker(database, defaultCfg())
	res, err := c.Check(context.Background(), m)
	if err != nil {
		t.Fatalf("Check: %v", err)
	}
	if len(res) != 0 {
		t.Fatalf("cross-scope facts must not resolve, got %+v", res)
	}
}

func TestPolicyWinnerDeterministicAcrossWriteOrder(t *testing.T) {
	now := time.Now().UTC()
	old := &db.Memory{ID: "a", Importance: 0.9, CreatedAt: now.Add(-time.Hour)}
	flaky := &db.Memory{ID: "b", Importance: 0.3, CreatedAt: now}

	// A written first, then B: the curated fact a wins.
	newWins, rule := policyWinner(flaky, old)
	if newWins || rule != "importance" {
		t.Errorf("expected a (importance) to win over b, got newWins=%v rule=%q", newWins, rule)
	}
	// B written first, then A: the curated fact a still wins — the
	// winner is independent of write order.
	newWins, rule = policyWinner(old, flaky)
	if !newWins || rule != "importance" {
		t.Errorf("expected a to win in the reverse order too, got newWins=%v rule=%q", newWins, rule)
	}
}

func TestPolicyWinnerTieBreaks(t *testing.T) {
	now := time.Now().UTC()
	// Equal importance: recency wins.
	newWins, rule := policyWinner(
		&db.Memory{ID: "new", Importance: 0.5, CreatedAt: now},
		&db.Memory{ID: "old", Importance: 0.5, CreatedAt: now.Add(-time.Hour)},
	)
	if !newWins || rule != "recency" {
		t.Errorf("expected newer write to win by recency, got newWins=%v rule=%q", newWins, rule)
	}
	// Same importance and timestamp: id breaks the tie deterministically.
	newWins, rule = policyWinner(
		&db.Memory{ID: "z-new", Importance: 0.5, CreatedAt: now},
		&db.Memory{ID: "a-old", Importance: 0.5, CreatedAt: now},
	)
	if newWins || rule != "id" {
		t.Errorf("expected lexicographically lower id to win the tie, got newWins=%v rule=%q", newWins, rule)
	}
}

func TestSupersedeDetailDeterministic(t *testing.T) {
	got := SupersedeDetail("winner-1", "actor-a", "actor-b", "recency", 0.9)
	want := `{"winner_id":"winner-1","winner_actor":"actor-a","loser_actor":"actor-b","rule":"recency","similarity":0.9}`
	if got != want {
		t.Errorf("SupersedeDetail = %s, want %s", got, want)
	}
}

func TestConflictPendingDetailDeterministic(t *testing.T) {
	got := ConflictPendingDetail("cand-1", "actor-c", 0.82)
	want := `{"candidate_id":"cand-1","candidate_actor":"actor-c","similarity":0.82}`
	if got != want {
		t.Errorf("ConflictPendingDetail = %s, want %s", got, want)
	}
}
