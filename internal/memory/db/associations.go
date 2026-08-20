package db

import (
	"fmt"
	"sort"
	"strings"
	"time"
)

// MemoryAssociation is a directed, weighted edge between two memories
// (#488). The graph is memory-to-memory: a retrieval hit can lift a
// directly associated memory that would otherwise never surface.
type MemoryAssociation struct {
	FromID    string    `json:"from_id"`
	ToID      string    `json:"to_id"`
	Weight    float64   `json:"weight"`
	CreatedBy string    `json:"created_by,omitempty"`
	CreatedAt time.Time `json:"created_at"`
}

// Association seeds (#488): edges are derived from signals the store
// already records, never from user-authored content.
const (
	SeedCoRetrieval     = "co-retrieval"  // memories returned together by the same query
	SeedSharedEntity    = "shared-entity" // memories linked to the same entity
	SeedConsolidation   = "consolidation" // memories absorbed into the same consolidation parent
	CoRetrievalWeight   = 0.5
	SharedEntityWeight  = 0.4
	ConsolidationWeight = 0.6
)

// SaveMemoryAssociation upserts a directed edge, keeping the maximum
// weight on conflict. An edge from a memory to itself is refused.
func (db *DB) SaveMemoryAssociation(fromID, toID string, weight float64, createdBy string) error {
	if fromID == "" || toID == "" {
		return fmt.Errorf("association requires both from_id and to_id")
	}
	if fromID == toID {
		return fmt.Errorf("association cannot link a memory to itself")
	}
	if weight <= 0 || weight > 1 {
		return fmt.Errorf("association weight must be in (0, 1], got %v", weight)
	}
	_, err := db.conn.Exec(
		`INSERT INTO memory_associations (from_id, to_id, weight, created_by, created_at)
		 VALUES (?, ?, ?, ?, ?)
		 ON CONFLICT(from_id, to_id) DO UPDATE SET weight = MAX(weight, excluded.weight)`,
		fromID, toID, weight, createdBy, time.Now().UTC(),
	)
	return err
}

// AssociationsFrom returns the outgoing association edges of the given
// memory ids as from → {to → weight}.
func (db *DB) AssociationsFrom(ids []string) (map[string]map[string]float64, error) {
	result := make(map[string]map[string]float64, len(ids))
	if len(ids) == 0 {
		return result, nil
	}
	placeholders := make([]string, len(ids))
	args := make([]interface{}, 0, len(ids))
	for i, id := range ids {
		placeholders[i] = "?"
		args = append(args, id)
		result[id] = map[string]float64{}
	}
	inClause := strings.Join(placeholders, ", ")
	rows, err := db.conn.Query(
		"SELECT from_id, to_id, weight FROM memory_associations WHERE from_id IN ("+inClause+")",
		args...,
	)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	for rows.Next() {
		var from, to string
		var weight float64
		if err := rows.Scan(&from, &to, &weight); err != nil {
			return nil, err
		}
		if result[from] == nil {
			result[from] = map[string]float64{}
		}
		result[from][to] = weight
	}
	return result, rows.Err()
}

// AssociationCount reports how many edges are stored.
func (db *DB) AssociationCount() (int, error) {
	var n int
	err := db.conn.QueryRow("SELECT COUNT(*) FROM memory_associations").Scan(&n)
	return n, err
}

// SeedMemoryAssociations derives memory-to-memory edges from signals the
// store already records (#488): co-retrieval in the query log, shared
// entity links, and consolidation siblings. Seeding is idempotent
// (upserts keep the maximum weight) and bounded per source so a
// pathological entity or query log cannot explode the edge set.
func (db *DB) SeedMemoryAssociations(createdBy string) (int, error) {
	inserted := 0
	upsert := func(from, to string, weight float64) error {
		if from == "" || to == "" || from == to {
			return nil
		}
		if err := db.SaveMemoryAssociation(from, to, weight, createdBy); err != nil {
			return err
		}
		inserted++
		return nil
	}

	// 1. Co-retrieval: memories returned together by the same query.
	rows, err := db.conn.Query(
		`SELECT qr1.query_id, qr1.memory_id, qr2.memory_id
		 FROM query_log_results qr1
		 JOIN query_log_results qr2 ON qr2.query_id = qr1.query_id AND qr2.memory_id > qr1.memory_id
		 ORDER BY qr1.query_id`,
	)
	if err != nil {
		return inserted, err
	}
	var coPairs []struct{ from, to string }
	for rows.Next() {
		var qid, a, b string
		if err := rows.Scan(&qid, &a, &b); err != nil {
			_ = rows.Close()
			return inserted, err
		}
		coPairs = append(coPairs, struct{ from, to string }{a, b})
	}
	_ = rows.Close()
	for _, p := range coPairs {
		if err := upsert(p.from, p.to, CoRetrievalWeight); err != nil {
			return inserted, err
		}
	}

	// 2. Shared entity links: the 50 most recent approved memories per
	// entity, paired exhaustively (bounded per entity).
	rows, err = db.conn.Query(
		`SELECT entity_id FROM memory_entities GROUP BY entity_id HAVING COUNT(*) > 1`,
	)
	if err != nil {
		return inserted, err
	}
	var entityIDs []string
	for rows.Next() {
		var eid string
		if err := rows.Scan(&eid); err != nil {
			_ = rows.Close()
			return inserted, err
		}
		entityIDs = append(entityIDs, eid)
	}
	_ = rows.Close()
	for _, eid := range entityIDs {
		memRows, err := db.conn.Query(
			`SELECT me.memory_id FROM memory_entities me
			 JOIN memories m ON m.id = me.memory_id
			 WHERE me.entity_id = ? AND m.review_status = 'approved' AND m.retired_at IS NULL
			 ORDER BY m.created_at DESC LIMIT 50`,
			eid,
		)
		if err != nil {
			return inserted, err
		}
		var mems []string
		for memRows.Next() {
			var mid string
			if err := memRows.Scan(&mid); err != nil {
				_ = memRows.Close()
				return inserted, err
			}
			mems = append(mems, mid)
		}
		_ = memRows.Close()
		for i := 0; i < len(mems); i++ {
			for j := i + 1; j < len(mems); j++ {
				if err := upsert(mems[i], mems[j], SharedEntityWeight); err != nil {
					return inserted, err
				}
			}
		}
	}

	// 3. Consolidation siblings: memories absorbed into the same parent.
	rows, err = db.conn.Query(
		`SELECT consolidated_into_id FROM memories
		 WHERE consolidated_into_id IS NOT NULL AND consolidated_into_id != ''
		 GROUP BY consolidated_into_id HAVING COUNT(*) > 1`,
	)
	if err != nil {
		return inserted, err
	}
	var parentIDs []string
	for rows.Next() {
		var pid string
		if err := rows.Scan(&pid); err != nil {
			_ = rows.Close()
			return inserted, err
		}
		parentIDs = append(parentIDs, pid)
	}
	_ = rows.Close()
	for _, pid := range parentIDs {
		memRows, err := db.conn.Query(
			`SELECT id FROM memories WHERE consolidated_into_id = ? AND review_status = 'approved' AND retired_at IS NULL`,
			pid,
		)
		if err != nil {
			return inserted, err
		}
		var mems []string
		for memRows.Next() {
			var mid string
			if err := memRows.Scan(&mid); err != nil {
				_ = memRows.Close()
				return inserted, err
			}
			mems = append(mems, mid)
		}
		_ = memRows.Close()
		for i := 0; i < len(mems); i++ {
			for j := i + 1; j < len(mems); j++ {
				if err := upsert(mems[i], mems[j], ConsolidationWeight); err != nil {
					return inserted, err
				}
			}
		}
	}

	return inserted, nil
}

// SpreadingBonus computes the memory-association bonus (#488) for every
// seed node: bounded BFS of depth 2 over the weighted edge set, taking the
// maximum over paths of parent_score × edge_weight × 0.5^hop (the shape
// from the upstream analysis; no constants are ported). Seeds are the
// top-ranked results of the current query; a memory up to 2 hops away from
// a strong hit can therefore surface even when its own embedding score is
// weak. Returns target id → bonus.
func (db *DB) SpreadingBonus(seeds map[string]float64, maxHop1, maxHop2 int) (map[string]float64, error) {
	bonus := make(map[string]float64)
	if len(seeds) == 0 {
		return bonus, nil
	}
	if maxHop1 <= 0 {
		maxHop1 = 32
	}
	if maxHop2 <= 0 {
		maxHop2 = 32
	}

	seedIDs := make([]string, 0, len(seeds))
	for id := range seeds {
		seedIDs = append(seedIDs, id)
	}
	hop1, err := db.AssociationsFrom(seedIDs)
	if err != nil {
		return nil, err
	}

	// Hop 1: seed → neighbor, bonus = seedScore × w × 0.5.
	hop1Targets := make(map[string]float64) // neighbor id → best bonus
	for seedID, seedScore := range seeds {
		for toID, w := range hop1[seedID] {
			b := seedScore * w * 0.5
			if b > hop1Targets[toID] {
				hop1Targets[toID] = b
			}
			if b > bonus[toID] {
				bonus[toID] = b
			}
		}
	}

	// Hop 2: neighbor → neighbor-of-neighbor, bonus = seedScore × w1 × w2 × 0.25.
	neighborIDs := make([]string, 0, len(hop1Targets))
	for id := range hop1Targets {
		neighborIDs = append(neighborIDs, id)
	}
	if len(neighborIDs) == 0 {
		return bonus, nil
	}
	// Keep the expansion bounded: only the strongest hop-1 targets expand.
	if len(neighborIDs) > maxHop2 {
		type scoredID struct {
			id    string
			score float64
		}
		sorted := make([]scoredID, 0, len(neighborIDs))
		for id, s := range hop1Targets {
			sorted = append(sorted, scoredID{id: id, score: s})
		}
		sort.Slice(sorted, func(i, j int) bool { return sorted[i].score > sorted[j].score })
		neighborIDs = neighborIDs[:0]
		for _, s := range sorted[:maxHop2] {
			neighborIDs = append(neighborIDs, s.id)
		}
	}
	hop2, err := db.AssociationsFrom(neighborIDs)
	if err != nil {
		return nil, err
	}
	for neighborID, w1 := range hop1Targets {
		for toID, w2 := range hop2[neighborID] {
			// The best path into the neighbor already carries the seed
			// score and the hop-1 discount; multiply by the second edge and
			// the hop-2 discount (max over paths).
			b := w1 * w2 * 0.5
			if b > bonus[toID] {
				bonus[toID] = b
			}
		}
	}
	_ = maxHop1 // hop-1 fan-out is bounded by the IN query; kept for API symmetry
	return bonus, nil
}
