package db

import (
	"fmt"
	"strings"
	"time"
)

// Canonical semantic kinds (#486). The taxonomy mirrors the four buckets
// documented in docs/agent-integration.md: preferences, project rules,
// architectural decisions, and constraints map onto these canonical values.
const (
	KindUser      = "user"      // user preferences and personal facts
	KindFeedback  = "feedback"  // feedback, corrections, evaluations
	KindProject   = "project"   // project rules, guidelines, architectural decisions, constraints
	KindReference = "reference" // reference material and external facts
)

// kindSynonyms snaps caller-supplied synonyms onto canonical kinds. Keys are
// lower-case; lookups normalize before matching.
var kindSynonyms = map[string]string{
	// user
	"user":        KindUser,
	"preference":  KindUser,
	"preferences": KindUser,
	"personal":    KindUser,
	"identity":    KindUser,
	"user-pref":   KindUser,
	"user-prefs":  KindUser,
	"about-user":  KindUser,
	// feedback
	"feedback":    KindFeedback,
	"correction":  KindFeedback,
	"corrections": KindFeedback,
	"critique":    KindFeedback,
	"evaluation":  KindFeedback,
	"review":      KindFeedback,
	"praise":      KindFeedback,
	"complaint":   KindFeedback,
	// project
	"project":                KindProject,
	"project-rule":           KindProject,
	"project-rules":          KindProject,
	"rule":                   KindProject,
	"rules":                  KindProject,
	"guideline":              KindProject,
	"guidelines":             KindProject,
	"constraint":             KindProject,
	"constraints":            KindProject,
	"architectural-decision": KindProject,
	"adr":                    KindProject,
	"decision":               KindProject,
	"decisions":              KindProject,
	"architecture":           KindProject,
	// reference
	"reference":     KindReference,
	"fact":          KindReference,
	"facts":         KindReference,
	"documentation": KindReference,
	"doc":           KindReference,
	"docs":          KindReference,
	"howto":         KindReference,
	"how-to":        KindReference,
	"api":           KindReference,
	"external":      KindReference,
}

// ValidKinds returns the canonical kind values in display order.
func ValidKinds() []string {
	return []string{KindUser, KindFeedback, KindProject, KindReference}
}

// NormalizeKind snaps a caller-supplied kind onto its canonical value.
// It returns (canonical, true) for known kinds and ("", false) for
// unclassifiable input. An empty input is unclassifiable: the write
// path decides whether that is a refusal or an unclassified row.
func NormalizeKind(kind string) (string, bool) {
	normalized := strings.ToLower(strings.TrimSpace(kind))
	if normalized == "" {
		return "", false
	}
	if canonical, ok := kindSynonyms[normalized]; ok {
		return canonical, true
	}
	// Tolerate dashed/snake variants of canonical values ("kind: user-preferences").
	compact := strings.NewReplacer("-", "", "_", "", " ", "").Replace(normalized)
	for _, canonical := range ValidKinds() {
		if compact == canonical {
			return canonical, true
		}
	}
	return "", false
}

// KindRank orders kinds for context assembly: identity-level facts
// (user) precede project chatter within the same score band. Unknown or
// unclassified kinds rank last.
func KindRank(kind string) int {
	switch kind {
	case KindUser:
		return 0
	case KindFeedback:
		return 1
	case KindProject:
		return 2
	case KindReference:
		return 3
	default:
		return 4
	}
}

// Review statuses (#485). 'approved' rows are live and retrievable;
// 'staged' rows are autonomous-write candidates excluded from retrieval
// until promoted or rejected.
const (
	ReviewApproved = "approved"
	ReviewStaged   = "staged"
)

// SetMemoryReviewStatus flips the review state of a memory (promote or
// re-stage). The status is validated against the known review states.
func (db *DB) SetMemoryReviewStatus(id, status string) error {
	if status != ReviewApproved && status != ReviewStaged {
		return fmt.Errorf("invalid review status %q (want %q or %q)", status, ReviewApproved, ReviewStaged)
	}
	res, err := db.conn.Exec("UPDATE memories SET review_status = ?, updated_at = ? WHERE id = ?", status, time.Now().UTC(), id)
	if err != nil {
		return err
	}
	if n, err := res.RowsAffected(); err == nil && n == 0 {
		return fmt.Errorf("memory not found: %s", id)
	}
	return nil
}

// SetMemoryKind assigns a canonical semantic kind to an existing memory.
func (db *DB) SetMemoryKind(id, kind string) error {
	canonical, ok := NormalizeKind(kind)
	if !ok {
		return fmt.Errorf("invalid kind %q (valid: %s)", kind, strings.Join(ValidKinds(), ", "))
	}
	res, err := db.conn.Exec("UPDATE memories SET kind = ?, updated_at = ? WHERE id = ?", canonical, time.Now().UTC(), id)
	if err != nil {
		return err
	}
	if n, err := res.RowsAffected(); err == nil && n == 0 {
		return fmt.Errorf("memory not found: %s", id)
	}
	return nil
}

// ListStagedMemories returns the review queue: memories written as
// candidates (#485) that are still awaiting promote/reject, newest first.
func (db *DB) ListStagedMemories(limit int) ([]*Memory, error) {
	if limit <= 0 {
		limit = 100
	}
	query := `SELECT id, content, scope, metadata, created_at, updated_at, created_by, updated_by, created_session, updated_session, consolidation_status, consolidated_into_id, importance, valid_from, valid_to, superseded_by, tier, expires_at, access_count, last_access, prev_access, review_status, kind, decay_factor, retired_at
		FROM memories WHERE review_status = 'staged' AND retired_at IS NULL ORDER BY created_at DESC LIMIT ?`
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
	return memories, nil
}

// SetDecayFactor stores the aging decay multiplier (#491) for one memory.
// Factors are always clamped to (0, 1] so a corrupt value can never boost
// a memory above its natural score.
func (db *DB) SetDecayFactor(id string, factor float64) error {
	if factor <= 0 || factor > 1 {
		return fmt.Errorf("decay factor %v out of range (0, 1]", factor)
	}
	res, err := db.conn.Exec("UPDATE memories SET decay_factor = ? WHERE id = ?", factor, id)
	if err != nil {
		return err
	}
	if n, err := res.RowsAffected(); err == nil && n == 0 {
		return fmt.Errorf("memory not found: %s", id)
	}
	return nil
}

// RetireMemory flags a memory as retired (#491). Retirement is a flag,
// never a hard delete: the row and its history survive.
func (db *DB) RetireMemory(id string, at time.Time) error {
	res, err := db.conn.Exec("UPDATE memories SET retired_at = ?, updated_at = ? WHERE id = ?", at.UTC(), time.Now().UTC(), id)
	if err != nil {
		return err
	}
	if n, err := res.RowsAffected(); err == nil && n == 0 {
		return fmt.Errorf("memory not found: %s", id)
	}
	return nil
}

// UnretireMemory clears the retirement flag so a memory becomes
// retrievable again at full decay (factor reset to 1.0).
func (db *DB) UnretireMemory(id string) error {
	res, err := db.conn.Exec("UPDATE memories SET retired_at = NULL, decay_factor = 1.0, updated_at = ? WHERE id = ?", time.Now().UTC(), id)
	if err != nil {
		return err
	}
	if n, err := res.RowsAffected(); err == nil && n == 0 {
		return fmt.Errorf("memory not found: %s", id)
	}
	return nil
}

// AgingCandidates returns every non-retired memory's aging inputs
// (created_at, access_count, last_access) for the aging pass (#491).
// Staged memories age like any other row; retirement is orthogonal to
// review state.
func (db *DB) AgingCandidates() ([]*Memory, error) {
	rows, err := db.conn.Query(`SELECT id, content, scope, metadata, created_at, updated_at, created_by, updated_by, created_session, updated_session, consolidation_status, consolidated_into_id, importance, valid_from, valid_to, superseded_by, tier, expires_at, access_count, last_access, prev_access, review_status, kind, decay_factor, retired_at
		FROM memories WHERE retired_at IS NULL`)
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
	return memories, nil
}
