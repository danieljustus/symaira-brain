package db

import (
	"math"
	"time"
)

// StalenessThreshold configures when a memory is considered stale for
// decay-aware consolidation.
type StalenessThreshold struct {
	// MinAccessCount: memories with access_count < this are candidates for deprioritization.
	// Zero means never-accessed memories are always considered stale.
	MinAccessCount int64
	// MaxDaysSinceAccess: memories whose last_access is older than this are stale.
	// Zero means no recency check (only access_count matters).
	MaxDaysSinceAccess float64
	// NeverAccessedPenalty: multiplier applied to consolidation priority when
	// a memory has never been accessed (last_access is NULL and access_count is 0).
	// Lower values = stronger deprioritization. 0 = exclude from consolidation entirely.
	NeverAccessedPenalty float64
	// StaleAccessPenalty: multiplier applied when access_count < MinAccessCount or
	// last_access is older than MaxDaysSinceAccess. 0 = exclude entirely.
	StaleAccessPenalty float64
}

// DefaultStalenessThreshold returns sensible defaults for decay-aware consolidation.
func DefaultStalenessThreshold() StalenessThreshold {
	return StalenessThreshold{
		MinAccessCount:       1,   // memories accessed 0 times are stale
		MaxDaysSinceAccess:   90,  // not accessed in 90 days = stale
		NeverAccessedPenalty: 0.1, // 90% reduction in consolidation priority
		StaleAccessPenalty:   0.3, // 70% reduction for stale memories
	}
}

// ConsolidationPriority computes a priority score for a memory to be
// considered for consolidation. Lower-scored memories should be consolidated
// first (they are less valuable or less likely to be needed).
//
// The priority is based on a combination of:
//   - access_count (frequently accessed memories are higher priority, kept longer)
//   - last_access (recently accessed memories are higher priority)
//   - importance (higher importance = higher priority)
//   - age (older memories are lower priority)
//
// Returns a score in [0, 1] where 0 = deprioritize (good consolidation target)
// and 1 = prioritize (keep as-is).
func (db *DB) ConsolidationPriority(m *Memory, threshold StalenessThreshold) float64 {
	if m == nil {
		return 0
	}

	// Base priority from importance.
	priority := m.Importance
	if priority <= 0 {
		priority = 0.5
	}

	// Access frequency boost: frequently accessed → higher priority.
	accessBoost := math.Log1p(float64(m.AccessCount)) / math.Log1p(100.0)
	if accessBoost > 1.0 {
		accessBoost = 1.0
	}

	// Apply staleness penalties.
	var penalty = 1.0
	neverAccessed := m.AccessCount == 0 && m.LastAccess == nil

	if neverAccessed {
		penalty = threshold.NeverAccessedPenalty
	} else if threshold.MinAccessCount > 0 && m.AccessCount < threshold.MinAccessCount {
		penalty = threshold.StaleAccessPenalty
	} else if threshold.MaxDaysSinceAccess > 0 && m.LastAccess != nil {
		daysSince := time.Since(*m.LastAccess).Hours() / 24.0
		if daysSince > threshold.MaxDaysSinceAccess {
			penalty = threshold.StaleAccessPenalty
		}
	}

	// Apply penalty to priority — stale memories get pushed down.
	priority *= penalty

	// Boost for recent last_access.
	if m.LastAccess != nil {
		daysSince := time.Since(*m.LastAccess).Hours() / 24.0
		if daysSince < 7 {
			// Accessed within the last week: full priority.
			priority *= 1.0 + accessBoost*0.5
		} else if daysSince < 30 {
			// Accessed within the last month: slight boost.
			priority *= 1.0 + accessBoost*0.2
		}
	} else if m.AccessCount > 0 {
		// Has access_count but no last_access record (upgraded from old data).
		// Apply a moderate access boost based on count only.
		priority *= 1.0 + accessBoost*0.1
	}

	// Age decay: older memories slowly lose priority.
	ageDays := time.Since(m.CreatedAt).Hours() / 24.0
	if ageDays > 730 {
		// More than 2 years: stronger decay.
		priority *= 0.7
	} else if ageDays > 365 {
		// More than a year old: slight decay.
		priority *= 0.9
	}

	// Clamp to [0, 1].
	if priority < 0 {
		priority = 0
	}
	if priority > 1 {
		priority = 1
	}

	return priority
}

// GetStaleMemories returns memories that are candidates for consolidation
// based on their access pattern. These are memories that have low access
// counts, old last_access timestamps, or have never been accessed.
// Returns memories sorted by ConsolidationPriority ascending (stalest first),
// limited to the given count.
func (db *DB) GetStaleMemories(limit int, threshold StalenessThreshold) ([]*Memory, error) {
	if limit <= 0 {
		limit = 100
	}

	// Use the lite query to avoid loading embeddings for stale candidates.
	query := `SELECT id, content, scope, metadata, created_at, updated_at,
		created_by, updated_by, created_session, updated_session,
		consolidation_status, consolidated_into_id, importance,
		valid_from, valid_to, superseded_by, tier, expires_at,
		access_count, last_access, prev_access, review_status, kind, decay_factor, retired_at
		FROM memories
		WHERE consolidation_status != 'archived'
		AND review_status = 'approved'
		AND retired_at IS NULL
		AND (tier != 'working' OR expires_at IS NULL OR expires_at > datetime('now'))
		ORDER BY access_count ASC, last_access ASC, created_at ASC
		LIMIT ?`

	rows, err := db.conn.Query(query, limit)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()

	var memories []*Memory
	for rows.Next() {
		m, err := scanMemoryLite(rows)
		if err != nil {
			return nil, err
		}
		memories = append(memories, m)
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}

	// Sort by priority (stalest first).
	type scored struct {
		m        *Memory
		priority float64
	}
	scoredMemories := make([]scored, len(memories))
	for i, m := range memories {
		scoredMemories[i] = scored{m: m, priority: db.ConsolidationPriority(m, threshold)}
	}

	// Sort by priority ascending (lowest = stalest = first).
	for i := 0; i < len(scoredMemories); i++ {
		for j := i + 1; j < len(scoredMemories); j++ {
			if scoredMemories[j].priority < scoredMemories[i].priority {
				scoredMemories[i], scoredMemories[j] = scoredMemories[j], scoredMemories[i]
			}
		}
	}

	result := make([]*Memory, 0, limit)
	for _, s := range scoredMemories {
		result = append(result, s.m)
	}

	return result, nil
}
