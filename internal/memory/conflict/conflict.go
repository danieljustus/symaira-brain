// Package conflict implements write-path contradiction detection (#462).
//
// Before a long-term memory is stored, prior memories in the same scope are
// recalled and compared. The check is tiered so that a plain CLI write
// never needs an LLM round-trip:
//
//  1. content-hash tier: a byte-identical repeat is deduplicated (no second
//     row is written, the existing row is returned);
//  2. cosine tier: candidates at or above NearDupThreshold are the same
//     fact region — the same threshold the consolidation engine uses — so
//     the newer representation replaces the older row (superseded_by +
//     closed valid_to, rule "near_dup"), exactly like consolidation's
//     newer-content-wins merge. Candidates in the
//     [ContradictionThreshold, NearDupThreshold) band need a verdict;
//  3. verdict tier (optional, off by default): one batched LLM call
//     classifies every band pair as repeat, contradiction or ambiguous. A
//     repeat resolves like the near-dup tier (newer representation wins),
//     a contradiction resolves with the deterministic winner policy.
//
// A confirmed contradiction resolves with a deterministic winner policy
// (importance, then recency, then id) and marks the loser via
// superseded_by while closing its valid_to, recording an audit event that
// names both memories, both actors and the deciding rule. Undecidable
// pairs are stored unchanged and surfaced with a conflict_pending audit
// event — a silently wrong supersession is worse than a visible conflict.
// Disabling the check restores the exact legacy write behavior.
package conflict

import (
	"context"
	"encoding/json"
	"fmt"
	"log/slog"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

// Audit action strings written by the conflict resolution path.
const (
	// ActionSupersede records a resolved contradiction: the loser row was
	// marked superseded_by the winner. The detail payload carries both
	// memory IDs, both actors and the deciding rule.
	ActionSupersede = "supersede"
	// ActionConflictPending records an undecided pair: both memories were
	// stored unchanged and the disagreement is surfaced for review.
	ActionConflictPending = "conflict_pending"
)

// Verdict classifies the relationship between a new memory and one
// candidate.
type Verdict int

const (
	// VerdictNone means the candidate is unrelated (below the
	// contradiction band) and no resolution exists for it.
	VerdictNone Verdict = iota
	// VerdictRepeat means the new content is the same fact as the
	// candidate: the write is deduplicated and no new row is created.
	VerdictRepeat
	// VerdictContradiction means the new content contradicts the
	// candidate: the loser is marked superseded by the winner.
	VerdictContradiction
	// VerdictAmbiguous means the check cannot decide with confidence:
	// both memories are stored unchanged and the pair is surfaced.
	VerdictAmbiguous
)

func (v Verdict) String() string {
	switch v {
	case VerdictNone:
		return "none"
	case VerdictRepeat:
		return "repeat"
	case VerdictContradiction:
		return "contradiction"
	case VerdictAmbiguous:
		return "ambiguous"
	default:
		return "unknown"
	}
}

// Pair is one contradiction-band pair handed to a VerdictProvider.
type Pair struct {
	Cand       *db.Memory
	NewContent string
	OldContent string
	NewActor   string
	OldActor   string
	Similarity float32
}

// VerdictProvider classifies contradiction-band pairs. Implementations
// must return exactly one verdict per pair, in input order. An error or a
// length mismatch degrades the whole batch to VerdictAmbiguous — a write
// never fails because the verdict tier failed.
type VerdictProvider interface {
	Verdicts(ctx context.Context, pairs []Pair) ([]Verdict, error)
}

// Resolution is the decided outcome for one candidate of a write.
type Resolution struct {
	Candidate  *db.Memory
	Similarity float32
	Verdict    Verdict
	// WinnerID/LoserID/WinnerActor/LoserActor/Rule are set for
	// VerdictContradiction. The winner is the memory that stays live;
	// the loser is marked superseded by it.
	WinnerID    string
	LoserID     string
	WinnerActor string
	LoserActor  string
	Rule        string // "importance" | "recency" | "id"
}

// Checker runs the tiered contradiction check for one new memory.
type Checker struct {
	db       *db.DB
	cfg      config.ConflictConfig
	verdicts VerdictProvider // nil = deterministic tiers only
}

// NewChecker builds a checker for the given configuration. The LLM verdict
// tier is enabled only when cfg.LLMProvider is non-empty.
func NewChecker(database *db.DB, cfg config.ConflictConfig) *Checker {
	cfg = normalize(cfg)
	c := &Checker{db: database, cfg: cfg}
	if cfg.LLMProvider != "" {
		c.verdicts = NewLLMVerdictProvider(cfg)
	}
	return c
}

// SetVerdictProvider overrides the verdict tier. Tests use this to inject
// a deterministic classifier.
func (c *Checker) SetVerdictProvider(p VerdictProvider) {
	c.verdicts = p
}

func normalize(cfg config.ConflictConfig) config.ConflictConfig {
	if cfg.ContradictionThreshold <= 0 || cfg.ContradictionThreshold >= 1 {
		cfg.ContradictionThreshold = 0.80
	}
	if cfg.NearDupThreshold <= 0 || cfg.NearDupThreshold >= 1 {
		cfg.NearDupThreshold = 0.95
	}
	if cfg.MaxCandidates <= 0 {
		cfg.MaxCandidates = 10
	}
	return cfg
}

// Check inspects the new memory against prior same-scope memories and
// returns one Resolution per relevant candidate. Callers apply the
// resolutions: a repeat replaces the write (keep the existing row), a
// contradiction resolves via supersession, an ambiguity is stored and
// surfaced. The returned resolutions are ordered by decreasing similarity.
//
// Degradation contract: an error never fails a write — it means "no
// decision", i.e. store both exactly like the legacy path. Missing
// embeddings or a disabled check short-circuit to no resolutions.
func (c *Checker) Check(ctx context.Context, m *db.Memory) ([]Resolution, error) {
	if c == nil || !c.cfg.Enabled {
		return nil, nil
	}
	if m == nil || len(m.Embedding) == 0 || m.EmbeddingSource == "" {
		return nil, nil
	}
	if m.CreatedAt.IsZero() {
		// Persistence would default this later; fix it now so the
		// recency policy compares against a deterministic timestamp.
		m.CreatedAt = time.Now().UTC()
	}
	hash := m.ContentHash
	if hash == "" {
		hash = db.ComputeContentHash(m.Content)
	}

	// Tier 1: byte-identical repeat in the same scope.
	dup, err := c.db.FindActiveDuplicate(hash, m.Scope)
	if err != nil {
		return nil, fmt.Errorf("conflict: duplicate lookup: %w", err)
	}
	if dup != nil {
		return []Resolution{{
			Candidate:  dup,
			Similarity: 1.0,
			Verdict:    VerdictRepeat,
		}}, nil
	}

	// Tier 2: cosine candidates in the same scope (LSH recall, ranked).
	results, err := c.db.SearchMemories(m.Embedding, m.EmbeddingSource, m.Scope, c.cfg.MaxCandidates)
	if err != nil {
		return nil, fmt.Errorf("conflict: candidate recall: %w", err)
	}

	var bestNearDup *Pair
	band := make([]Pair, 0, len(results))
	for _, r := range results {
		cand := r.Memory
		if cand == nil || cand.ID == m.ID || cand.SupersededBy != "" {
			continue
		}
		if len(cand.Embedding) == 0 {
			continue
		}
		sim := db.CosineSimilarity(m.Embedding, cand.Embedding)
		switch {
		case sim >= float32(c.cfg.NearDupThreshold):
			// Same fact region (the consolidation engine's same-fact
			// threshold): like consolidation, the newer representation
			// replaces the older one. The closest candidate is the
			// dedup target.
			if bestNearDup == nil || sim > bestNearDup.Similarity {
				bestNearDup = &Pair{
					Cand:       cand,
					NewContent: m.Content,
					OldContent: cand.Content,
					NewActor:   m.CreatedBy,
					OldActor:   cand.CreatedBy,
					Similarity: sim,
				}
			}
		case sim >= float32(c.cfg.ContradictionThreshold):
			band = append(band, Pair{
				Cand:       cand,
				NewContent: m.Content,
				OldContent: cand.Content,
				NewActor:   m.CreatedBy,
				OldActor:   cand.CreatedBy,
				Similarity: sim,
			})
		}
	}

	// A near-duplicate short-circuits the band: the new content is the
	// same fact as an existing row, so it replaces that row (newer
	// representation wins, matching consolidation) and nothing else is
	// litigated. Byte-identical content never reaches this tier — the
	// hash tier above already deduplicated it.
	if bestNearDup != nil {
		p := *bestNearDup
		return []Resolution{{
			Candidate:   p.Cand,
			Similarity:  p.Similarity,
			Verdict:     VerdictContradiction,
			WinnerID:    m.ID,
			LoserID:     p.Cand.ID,
			WinnerActor: m.CreatedBy,
			LoserActor:  p.Cand.CreatedBy,
			Rule:        "near_dup",
		}}, nil
	}
	if len(band) == 0 {
		return nil, nil
	}

	// Tier 3: verdicts for the contradiction band.
	var verdicts []Verdict
	if c.verdicts != nil {
		verdicts, err = c.verdicts.Verdicts(ctx, band)
		if err != nil || len(verdicts) != len(band) {
			slog.Warn("conflict: verdict tier failed, storing both", "pairs", len(band), "error", err)
			verdicts = nil
		}
	}
	if verdicts == nil {
		verdicts = make([]Verdict, len(band))
		for i := range verdicts {
			verdicts[i] = VerdictAmbiguous
		}
	}

	res := make([]Resolution, 0, len(band))
	for i, v := range verdicts {
		p := band[i]
		switch v {
		case VerdictContradiction:
			newWins, rule := policyWinner(m, p.Cand)
			r := Resolution{
				Candidate:  p.Cand,
				Similarity: p.Similarity,
				Verdict:    VerdictContradiction,
				Rule:       rule,
			}
			if newWins {
				r.WinnerID, r.LoserID = m.ID, p.Cand.ID
				r.WinnerActor, r.LoserActor = m.CreatedBy, p.Cand.CreatedBy
			} else {
				r.WinnerID, r.LoserID = p.Cand.ID, m.ID
				r.WinnerActor, r.LoserActor = p.Cand.CreatedBy, m.CreatedBy
			}
			res = append(res, r)
		case VerdictRepeat:
			// The LLM classified the pair as the same claim, reworded:
			// like the near-dup tier, the newer representation replaces
			// the older row.
			res = append(res, Resolution{
				Candidate:   p.Cand,
				Similarity:  p.Similarity,
				Verdict:     VerdictContradiction,
				WinnerID:    m.ID,
				LoserID:     p.Cand.ID,
				WinnerActor: m.CreatedBy,
				LoserActor:  p.Cand.CreatedBy,
				Rule:        "repeat",
			})
		default:
			res = append(res, Resolution{Candidate: p.Cand, Similarity: p.Similarity, Verdict: VerdictAmbiguous})
		}
	}
	return res, nil
}

// policyWinner picks the winner of a confirmed contradiction. The policy
// is a deterministic total order — the same pair of writes in the same
// order always resolves identically:
//
//  1. importance (higher wins): a curated fact outranks a flaky overwrite,
//     protecting the store from a silently wrong supersession;
//  2. recency (newer wins): the most recent assertion reflects current
//     truth;
//  3. id (lexicographically lower wins): stable final tie-break.
//
// Client trust is intentionally absent as an ordering input: #455
// delivered client attribution, not a per-client trust model, so inventing
// one here would be guesswork (see #462). Both actors are still recorded
// in the audit event, which keeps every resolution reviewable and
// reversible by hand.
func policyWinner(newM, cand *db.Memory) (newWins bool, rule string) {
	if newM.Importance != cand.Importance {
		return newM.Importance > cand.Importance, "importance"
	}
	if !newM.CreatedAt.Equal(cand.CreatedAt) {
		return newM.CreatedAt.After(cand.CreatedAt), "recency"
	}
	return newM.ID < cand.ID, "id"
}

type supersedeDetail struct {
	WinnerID    string  `json:"winner_id"`
	WinnerActor string  `json:"winner_actor"`
	LoserActor  string  `json:"loser_actor"`
	Rule        string  `json:"rule"`
	Similarity  float32 `json:"similarity"`
}

// SupersedeDetail renders the audit detail payload for a resolved
// contradiction: the winner, both actors and the deciding rule. Field
// order is fixed so the payload is byte-deterministic for tests.
func SupersedeDetail(winnerID, winnerActor, loserActor, rule string, similarity float32) string {
	b, err := json.Marshal(supersedeDetail{
		WinnerID:    winnerID,
		WinnerActor: winnerActor,
		LoserActor:  loserActor,
		Rule:        rule,
		Similarity:  similarity,
	})
	if err != nil {
		return fmt.Sprintf(`{"winner_id":%q,"rule":%q}`, winnerID, rule)
	}
	return string(b)
}

type conflictPendingDetail struct {
	CandidateID    string  `json:"candidate_id"`
	CandidateActor string  `json:"candidate_actor"`
	Similarity     float32 `json:"similarity"`
}

// ConflictPendingDetail renders the audit detail payload for an undecided
// pair that was stored unchanged.
func ConflictPendingDetail(candidateID, candidateActor string, similarity float32) string {
	b, err := json.Marshal(conflictPendingDetail{
		CandidateID:    candidateID,
		CandidateActor: candidateActor,
		Similarity:     similarity,
	})
	if err != nil {
		return fmt.Sprintf(`{"candidate_id":%q}`, candidateID)
	}
	return string(b)
}
