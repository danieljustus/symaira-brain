package db

import (
	"database/sql"
	"fmt"
	"sort"
	"strings"
	"time"
	"unicode"
)

const maxCandidates = 2000

// allowedFTSScopes is the set of scope values permitted in FTS5 queries.
// Must match security.ValidScopes to prevent FTS5 injection via the scope parameter.
var allowedFTSScopes = map[string]bool{
	"":        true,
	"global":  true,
	"project": true,
	"agent":   true,
	"user":    true,
	"session": true,
}

var stopWords = map[string]bool{
	"a": true, "an": true, "the": true, "is": true, "are": true, "was": true,
	"were": true, "be": true, "been": true, "being": true, "have": true, "has": true,
	"had": true, "do": true, "does": true, "did": true, "will": true, "would": true,
	"could": true, "should": true, "may": true, "might": true, "shall": true,
	"can": true, "to": true, "of": true, "in": true, "for": true, "on": true,
	"with": true, "at": true, "by": true, "from": true, "as": true, "into": true,
	"through": true, "during": true, "before": true, "after": true, "above": true,
	"below": true, "between": true, "out": true, "off": true, "over": true,
	"under": true, "again": true, "further": true, "then": true, "once": true,
	"and": true, "but": true, "or": true, "nor": true, "not": true, "so": true,
	"yet": true, "both": true, "either": true, "neither": true, "each": true,
	"every": true, "all": true, "any": true, "few": true, "more": true,
	"most": true, "other": true, "some": true, "such": true, "no": true,
	"only": true, "own": true, "same": true, "than": true, "too": true,
	"very": true, "just": true, "because": true, "if": true, "when": true,
	"where": true, "how": true, "what": true, "which": true, "who": true,
	"whom": true, "this": true, "that": true, "these": true, "those": true,
	"i": true, "me": true, "my": true, "we": true, "our": true, "you": true,
	"your": true, "he": true, "him": true, "his": true, "she": true, "her": true,
	"it": true, "its": true, "they": true, "them": true, "their": true,
}

// Tokenize splits text into lowercased tokens, filtering stop words and short words.
func Tokenize(text string) []string {
	var tokens []string
	var current strings.Builder
	for _, r := range strings.ToLower(text) {
		if unicode.IsLetter(r) || unicode.IsDigit(r) {
			current.WriteRune(r)
		} else if current.Len() > 0 {
			word := current.String()
			current.Reset()
			if !stopWords[word] && len(word) > 1 {
				tokens = append(tokens, word)
			}
		}
	}
	if current.Len() > 0 {
		word := current.String()
		if !stopWords[word] && len(word) > 1 {
			tokens = append(tokens, word)
		}
	}
	return tokens
}

// HybridResult extends SearchResult with hybrid scoring details.
type HybridResult struct {
	Memory      *Memory `json:"memory"`
	VectorScore float32 `json:"vector_score"`
	BM25Score   float64 `json:"bm25_score"`
	FusedScore  float64 `json:"fused_score"`
}

// ReciprocalRankFusion combines ranked lists using RRF: score = sum(1/(k+rank_i)).
// k=60 is the standard constant.
func ReciprocalRankFusion(rankedLists [][]string, k int) map[string]float64 {
	if k <= 0 {
		k = 60
	}
	scores := make(map[string]float64)
	for _, list := range rankedLists {
		for rank, id := range list {
			scores[id] += 1.0 / float64(k+rank+1)
		}
	}
	return scores
}

// Sparsemax computes the α=2 entmax (sparsemax) over the given score vector.
// Returns a sparse vector where low-scoring elements are exactly zero and the
// remaining positive entries sum to 1 (a probability distribution on the support).
// Reference: Martins & Astudillo (2016), "From Softmax to Sparsemax".
func Sparsemax(scores []float64) []float64 {
	if len(scores) == 0 {
		return nil
	}

	n := len(scores)
	type pair struct {
		score float64
		idx   int
	}
	sorted := make([]pair, n)
	for i, s := range scores {
		sorted[i] = pair{score: s, idx: i}
	}
	sort.Slice(sorted, func(i, j int) bool {
		return sorted[i].score > sorted[j].score
	})

	result := make([]float64, n)
	var cumSum float64

	for k := 1; k <= n; k++ {
		cumSum += sorted[k-1].score
		tau := (cumSum - 1) / float64(k)

		if k == n || sorted[k].score <= tau {
			// threshold found; apply sparsemax(z)_i = max(0, z_i - tau)
			for j := 0; j < k; j++ {
				val := sorted[j].score - tau
				if val > 0 {
					result[sorted[j].idx] = val
				}
			}
			return result
		}
	}

	return result
}

// SearchMemoriesBM25 performs keyword-based search using SQLite FTS5 BM25 scoring.
func (db *DB) SearchMemoriesBM25(query string, scope string, limit int, timeWindow ...TimeWindow) ([]SearchResult, error) {
	start := time.Now()
	if !allowedFTSScopes[scope] {
		return nil, fmt.Errorf("invalid search scope %q: must be one of global, project, agent, user, session", scope)
	}

	queryTerms := Tokenize(query)
	if len(queryTerms) == 0 {
		latency := time.Since(start)
		db.retrievalStats.Record(0, 0, latency)
		return nil, nil
	}

	tw := TimeWindow{}
	if len(timeWindow) > 0 {
		tw = timeWindow[0]
	}
	twClause, twArgs := TimeWindowClause(tw, "m")

	var ftsQuery string
	if scope != "" {
		ftsQuery = "scope:" + scope + " AND (" + strings.Join(queryTerms, " OR ") + ")"
	} else {
		ftsQuery = strings.Join(queryTerms, " OR ")
	}

	baseSQL := `SELECT m.id, m.content, m.scope, m.metadata, m.created_at, m.updated_at,
		        m.created_by, m.updated_by, m.created_session, m.updated_session,
		        m.consolidation_status, m.consolidated_into_id, m.importance,
		        m.valid_from, m.valid_to, m.superseded_by,
		        m.access_count, m.last_access, m.prev_access, m.review_status, m.kind, m.decay_factor, m.retired_at,
		        rank
		 FROM memories_fts fts
		 JOIN memories m ON fts.id = m.id
		 WHERE memories_fts MATCH ?` + twClause + `
		   AND m.review_status = 'approved'
		   AND m.retired_at IS NULL
		 ORDER BY rank
		 LIMIT ?`

	args := make([]interface{}, 0, 2+len(twArgs))
	args = append(args, ftsQuery)
	args = append(args, twArgs...)
	args = append(args, limit)

	rows, err := db.conn.Query(baseSQL, args...)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()

	var results []SearchResult
	for rows.Next() {
		var m Memory
		var metaStr string
		var consolidatedInto sql.NullString
		var validFrom, validTo sql.NullTime
		var supersededBy sql.NullString
		var rank float64
		var accessCount sql.NullInt64
		var lastAccess sql.NullTime
		var prevAccess sql.NullTime
		var reviewStatus, kind string
		var decayFactor float64
		var retiredAt sql.NullTime
		if err := rows.Scan(&m.ID, &m.Content, &m.Scope, &metaStr, &m.CreatedAt, &m.UpdatedAt,
			&m.CreatedBy, &m.UpdatedBy, &m.CreatedSession, &m.UpdatedSession,
			&m.ConsolidationStatus, &consolidatedInto, &m.Importance,
			&validFrom, &validTo, &supersededBy, &accessCount, &lastAccess, &prevAccess,
			&reviewStatus, &kind, &decayFactor, &retiredAt, &rank); err != nil {
			return nil, err
		}
		m.ReviewStatus = reviewStatus
		m.Kind = kind
		m.DecayFactor = decayFactor
		if retiredAt.Valid {
			m.RetiredAt = &retiredAt.Time
		}
		if prevAccess.Valid {
			m.PrevAccess = &prevAccess.Time
		}
		if err := populateMemoryFields(&m, metaStr, consolidatedInto, validFrom, validTo, supersededBy); err != nil {
			return nil, err
		}
		if accessCount.Valid {
			m.AccessCount = accessCount.Int64
		}
		if lastAccess.Valid {
			m.LastAccess = &lastAccess.Time
		}
		results = append(results, SearchResult{
			Memory: &m,
			Score:  float32(-rank),
		})
	}

	latency := time.Since(start)
	if err := rows.Err(); err != nil {
		return nil, err
	}
	if len(results) > 0 {
		db.retrievalStats.Record(len(results), float64(results[0].Score), latency)
	} else {
		db.retrievalStats.Record(0, 0, latency)
	}
	return results, nil
}

// HybridSearch combines vector similarity and BM25 keyword search using
// Reciprocal Rank Fusion, with optional sparsemax sparsification. Supports
// entity, trust, and policy filtering on the vector arm, and an optional
// time window on the BM25 arm. Returns results ranked by fused score,
// trimmed to the hard bound of limit.
func (db *DB) HybridSearch(queryVec []float32, querySource string, queryText string, scope string, limit int, entityID string, trustFilter TrustFilter, policyFilter PolicyFilter, vectorWeight, bm25Weight float64, timeWindow ...TimeWindow) ([]HybridResult, error) {
	return db.HybridSearchWithWeights(queryVec, querySource, queryText, scope, limit, entityID, trustFilter, policyFilter, vectorWeight, bm25Weight, RankingWeights{}, timeWindow...)
}

// HybridSearchWithWeights is HybridSearch with explicit ranking weights
// (spreading bonus #488, spacing-aware reinforcement #489). An empty
// RankingWeights keeps the defaults.
func (db *DB) HybridSearchWithWeights(queryVec []float32, querySource string, queryText string, scope string, limit int, entityID string, trustFilter TrustFilter, policyFilter PolicyFilter, vectorWeight, bm25Weight float64, weights RankingWeights, timeWindow ...TimeWindow) ([]HybridResult, error) {
	pam := db.perArmMultiplier
	if pam <= 0 {
		pam = 3
	}
	candidateLimit := limit * pam
	if candidateLimit > maxCandidates {
		candidateLimit = maxCandidates
	}

	tw := TimeWindow{}
	if len(timeWindow) > 0 {
		tw = timeWindow[0]
	}

	// Vector arm with full filtering (entity, trust, policy)
	vectorResults, err := db.SearchMemoriesFilteredWithTrust(queryVec, querySource, scope, candidateLimit, entityID, trustFilter, policyFilter, tw, "", weights)
	if err != nil {
		return nil, err
	}

	// BM25 arm (scope-filtered only; entity/trust/policy filtering is
	// applied at the vector side and naturally down-ranks non-matching
	// results through RRF fusion)
	bm25Results, err := db.SearchMemoriesBM25(queryText, scope, candidateLimit, tw)
	if err != nil {
		return nil, err
	}

	// Build ID lists for RRF
	var vectorList, bm25List []string
	for _, r := range vectorResults {
		vectorList = append(vectorList, r.Memory.ID)
	}
	for _, r := range bm25Results {
		bm25List = append(bm25List, r.Memory.ID)
	}

	rrfScores := ReciprocalRankFusion([][]string{vectorList, bm25List}, 60)

	// Index by ID
	memByID := make(map[string]*Memory)
	vecScoreByID := make(map[string]float32)
	for _, r := range vectorResults {
		memByID[r.Memory.ID] = r.Memory
		vecScoreByID[r.Memory.ID] = r.Score
	}
	bm25ScoreByID := make(map[string]float64)
	for _, r := range bm25Results {
		if _, exists := memByID[r.Memory.ID]; !exists {
			memByID[r.Memory.ID] = r.Memory
		}
		bm25ScoreByID[r.Memory.ID] = float64(r.Score)
	}

	type fused struct {
		id         string
		fusedScore float64
	}
	all := make([]fused, 0, len(rrfScores))
	for id, rrf := range rrfScores {
		vecS := float64(vecScoreByID[id])
		bm25S := bm25ScoreByID[id]
		// Aging decay (#491) applies to the fused score: a memory that the
		// aging pass decayed below its natural weight competes fairly on
		// both arms. The vector arm already carries the decayed score;
		// memories only present via BM25 get the multiplier here.
		decay := 1.0
		if m, ok := memByID[id]; ok {
			if d := m.DecayFactor; d > 0 && d <= 1 {
				decay = d
			}
		}
		fusedScore := (vectorWeight*vecS + bm25Weight*bm25S + (1-vectorWeight-bm25Weight)*rrf) * decay
		all = append(all, fused{id: id, fusedScore: fusedScore})
	}
	sort.Slice(all, func(i, j int) bool {
		return all[i].fusedScore > all[j].fusedScore
	})

	// Optional sparsemax sparsification (α=2)
	if db.sparsemaxEnabled && len(all) > 0 {
		scores := make([]float64, len(all))
		for i, f := range all {
			scores[i] = f.fusedScore
		}
		sparseScores := Sparsemax(scores)
		kept := make([]fused, 0, len(all))
		for i, f := range all {
			if sparseScores[i] > 0 {
				f.fusedScore = sparseScores[i]
				kept = append(kept, f)
			}
		}
		all = kept
		// Re-sort by sparsemax score (should already be in order, but be safe)
		sort.Slice(all, func(i, j int) bool {
			return all[i].fusedScore > all[j].fusedScore
		})
	}

	// Hard bound: limit
	if limit > len(all) {
		limit = len(all)
	}

	results := make([]HybridResult, 0, limit)
	for i := 0; i < limit; i++ {
		id := all[i].id
		results = append(results, HybridResult{
			Memory:      memByID[id],
			VectorScore: vecScoreByID[id],
			BM25Score:   bm25ScoreByID[id],
			FusedScore:  all[i].fusedScore,
		})
	}
	return results, nil
}
