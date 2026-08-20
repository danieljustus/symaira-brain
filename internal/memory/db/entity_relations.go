package db

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"strings"
	"time"

	"github.com/google/uuid"
)

// MaxGraphDepth bounds how far GraphNeighbors will traverse, so a query
// against a large, densely connected graph cannot run away.
const MaxGraphDepth = 3

// Relation provenance verification states.
const (
	VerificationVerified   = "verified"
	VerificationUnverified = "unverified"
)

// MaxEvidenceBytes bounds the raw evidence JSON blob so a caller cannot
// attach an unbounded payload (e.g. a full transcript) to a relation.
const MaxEvidenceBytes = 4096

// MaxSourceDocIDLength bounds the evidence source_doc_id field.
const MaxSourceDocIDLength = 200

// EntityRelation is a directed, typed relationship between two entities,
// e.g. "Daniel works-with Musterland Bank". ID, Source, SourceRef,
// Verification, Evidence and UpdatedAt are additive provenance fields:
// relations created before they existed read back with ID backfilled and
// the rest at their zero value.
type EntityRelation struct {
	ID           string     `json:"id"`
	FromEntityID string     `json:"from_entity_id"`
	ToEntityID   string     `json:"to_entity_id"`
	RelationType string     `json:"relation_type"`
	Source       string     `json:"source,omitempty"`
	SourceRef    string     `json:"source_ref,omitempty"`
	Verification string     `json:"verification,omitempty"`
	Evidence     string     `json:"evidence,omitempty"`
	CreatedBy    string     `json:"created_by,omitempty"`
	CreatedAt    time.Time  `json:"created_at"`
	UpdatedAt    time.Time  `json:"updated_at"`
	ValidFrom    *time.Time `json:"valid_from,omitempty"`
	ValidUntil   *time.Time `json:"valid_until,omitempty"`
}

// RelationEvidence is optional, bounded provenance detail attached to a
// relation: a reference to the source document plus an optional time span
// (e.g. audio/video timestamps) or character span (e.g. transcript
// offsets). Fields are pointers so "absent" is distinguishable from zero.
type RelationEvidence struct {
	SourceDocID string   `json:"source_doc_id,omitempty"`
	TimeStart   *float64 `json:"time_start,omitempty"`
	TimeEnd     *float64 `json:"time_end,omitempty"`
	CharStart   *int     `json:"char_start,omitempty"`
	CharEnd     *int     `json:"char_end,omitempty"`
}

// ValidateRelationEvidence parses and validates a raw evidence JSON blob,
// rejecting invalid, malformed or oversized input before it ever reaches
// the database. An empty (or whitespace-only) string is valid and means "no
// evidence attached". On success it returns the canonicalized JSON to
// store — unknown fields are dropped so arbitrary payloads cannot be
// smuggled in through extra keys.
func ValidateRelationEvidence(raw string) (string, error) {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return "", nil
	}
	if len(raw) > MaxEvidenceBytes {
		return "", fmt.Errorf("evidence exceeds maximum size of %d bytes", MaxEvidenceBytes)
	}

	var ev RelationEvidence
	if err := json.Unmarshal([]byte(raw), &ev); err != nil {
		return "", fmt.Errorf("invalid evidence JSON: %w", err)
	}
	if len(ev.SourceDocID) > MaxSourceDocIDLength {
		return "", fmt.Errorf("evidence source_doc_id exceeds maximum length of %d characters", MaxSourceDocIDLength)
	}
	if ev.CharStart != nil && *ev.CharStart < 0 {
		return "", fmt.Errorf("evidence char_start must be non-negative")
	}
	if ev.CharEnd != nil && *ev.CharEnd < 0 {
		return "", fmt.Errorf("evidence char_end must be non-negative")
	}
	if ev.CharStart != nil && ev.CharEnd != nil && *ev.CharEnd < *ev.CharStart {
		return "", fmt.Errorf("evidence char_end must be >= char_start")
	}
	if ev.TimeStart != nil && *ev.TimeStart < 0 {
		return "", fmt.Errorf("evidence time_start must be non-negative")
	}
	if ev.TimeEnd != nil && *ev.TimeEnd < 0 {
		return "", fmt.Errorf("evidence time_end must be non-negative")
	}
	if ev.TimeStart != nil && ev.TimeEnd != nil && *ev.TimeEnd < *ev.TimeStart {
		return "", fmt.Errorf("evidence time_end must be >= time_start")
	}

	canon, err := json.Marshal(ev)
	if err != nil {
		return "", fmt.Errorf("failed to canonicalize evidence: %w", err)
	}
	return string(canon), nil
}

// VerifiedRelationConflictError indicates a provenance-aware relation write
// would have overwritten an already-verified relation with different
// provenance. The caller must resolve the conflict explicitly instead of it
// happening silently.
type VerifiedRelationConflictError struct {
	FromEntityID string
	ToEntityID   string
	RelationType string
}

func (e *VerifiedRelationConflictError) Error() string {
	return fmt.Sprintf("relation %s --%s--> %s is already verified with different provenance; refusing to overwrite", e.FromEntityID, e.RelationType, e.ToEntityID)
}

// SaveEntityRelation creates a directed relation between two entities. It is
// idempotent: creating the same (from, to, type) triple again is a no-op
// that leaves any existing row (including its provenance) untouched.
func (db *DB) SaveEntityRelation(r *EntityRelation) error {
	if r.CreatedAt.IsZero() {
		r.CreatedAt = time.Now().UTC()
	}
	if r.UpdatedAt.IsZero() {
		r.UpdatedAt = r.CreatedAt
	}
	if r.ID == "" {
		r.ID = uuid.New().String()
	}
	existing, err := db.getRelationByTriple(r.FromEntityID, r.ToEntityID, r.RelationType)
	if err != nil {
		return err
	}
	if existing != nil {
		return nil
	}
	_, err = db.conn.Exec(
		`INSERT INTO entity_relations
			(id, from_entity_id, to_entity_id, relation_type, source, source_ref, verification, evidence, created_by, created_at, updated_at, valid_from, valid_until)
			VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
		r.ID, r.FromEntityID, r.ToEntityID, r.RelationType, r.Source, r.SourceRef, r.Verification, r.Evidence, r.CreatedBy, r.CreatedAt, r.UpdatedAt, r.ValidFrom, r.ValidUntil,
	)
	if err != nil {
		return err
	}
	db.auditRelation("relation_create", r)
	return nil
}

// SaveEntityRelationProvenance creates or updates a directed relation with
// caller-supplied provenance (source, source_ref, verification, evidence).
// Idempotency is scoped to (source, source_ref) on top of the (from, to,
// type) triple: retrying the exact same call returns the existing relation
// unchanged. When the triple already exists with different provenance, an
// already-verified relation is never silently overwritten —
// *VerifiedRelationConflictError is returned instead. An existing
// unverified (or provenance-less legacy) relation is enriched in place.
// r.Evidence is validated via ValidateRelationEvidence before any mutation.
func (db *DB) SaveEntityRelationProvenance(r *EntityRelation) (*EntityRelation, error) {
	if r.Verification != "" && r.Verification != VerificationVerified && r.Verification != VerificationUnverified {
		return nil, fmt.Errorf("invalid verification %q: must be %q, %q, or empty", r.Verification, VerificationVerified, VerificationUnverified)
	}
	canonEvidence, err := ValidateRelationEvidence(r.Evidence)
	if err != nil {
		return nil, err
	}

	existing, err := db.getRelationByTriple(r.FromEntityID, r.ToEntityID, r.RelationType)
	if err != nil {
		return nil, err
	}

	now := time.Now().UTC()

	if existing == nil {
		saved := *r
		saved.Evidence = canonEvidence
		if saved.ID == "" {
			saved.ID = uuid.New().String()
		}
		saved.CreatedAt = now
		saved.UpdatedAt = now
		_, err := db.conn.Exec(
			`INSERT INTO entity_relations
				(id, from_entity_id, to_entity_id, relation_type, source, source_ref, verification, evidence, created_by, created_at, updated_at, valid_from, valid_until)
				VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
			saved.ID, saved.FromEntityID, saved.ToEntityID, saved.RelationType, saved.Source, saved.SourceRef, saved.Verification, saved.Evidence, saved.CreatedBy, saved.CreatedAt, saved.UpdatedAt, saved.ValidFrom, saved.ValidUntil,
		)
		if err != nil {
			return nil, err
		}
		db.auditRelation("relation_create", &saved)
		return &saved, nil
	}

	// Identical retry: same source + source_ref on the same triple. Return
	// the existing relation unchanged rather than mutating it again.
	if existing.Source == r.Source && existing.SourceRef == r.SourceRef {
		return existing, nil
	}

	// Version chain: when the caller supplies a valid_from on a new version
	// of the same triple, update the row in place with the new provenance
	// and temporal fields. The "close" of the previous interval is implicit
	// in the new valid_from — callers querying as-of a date will see the
	// old interval as expired when valid_from moves forward.
	if r.ValidFrom != nil {
		updated := *existing
		updated.Source = r.Source
		updated.SourceRef = r.SourceRef
		updated.Verification = r.Verification
		updated.Evidence = canonEvidence
		if r.CreatedBy != "" {
			updated.CreatedBy = r.CreatedBy
		}
		updated.ValidFrom = r.ValidFrom
		updated.ValidUntil = r.ValidUntil
		updated.UpdatedAt = now

		_, err := db.conn.Exec(
			`UPDATE entity_relations SET source = ?, source_ref = ?, verification = ?, evidence = ?, created_by = ?, updated_at = ?, valid_from = ?, valid_until = ?
				WHERE id = ?`,
			updated.Source, updated.SourceRef, updated.Verification, updated.Evidence, updated.CreatedBy, updated.UpdatedAt, updated.ValidFrom, updated.ValidUntil,
			existing.ID,
		)
		if err != nil {
			return nil, err
		}
		db.auditRelation("relation_update", &updated)
		return &updated, nil
	}

	if existing.Verification == VerificationVerified {
		return nil, &VerifiedRelationConflictError{FromEntityID: r.FromEntityID, ToEntityID: r.ToEntityID, RelationType: r.RelationType}
	}

	updated := *existing
	updated.Source = r.Source
	updated.SourceRef = r.SourceRef
	updated.Verification = r.Verification
	updated.Evidence = canonEvidence
	if r.CreatedBy != "" {
		updated.CreatedBy = r.CreatedBy
	}
	updated.UpdatedAt = now

	_, err = db.conn.Exec(
		`UPDATE entity_relations SET source = ?, source_ref = ?, verification = ?, evidence = ?, created_by = ?, updated_at = ?, valid_from = ?, valid_until = ?
			WHERE from_entity_id = ? AND to_entity_id = ? AND relation_type = ?`,
		updated.Source, updated.SourceRef, updated.Verification, updated.Evidence, updated.CreatedBy, updated.UpdatedAt, updated.ValidFrom, updated.ValidUntil,
		r.FromEntityID, r.ToEntityID, r.RelationType,
	)
	if err != nil {
		return nil, err
	}
	db.auditRelation("relation_update", &updated)
	return &updated, nil
}

// DeleteEntityRelation removes a specific directed relation.
func (db *DB) DeleteEntityRelation(fromEntityID, toEntityID, relationType string) error {
	existing, err := db.getRelationByTriple(fromEntityID, toEntityID, relationType)
	if err != nil {
		return err
	}
	_, err = db.conn.Exec(
		"DELETE FROM entity_relations WHERE from_entity_id = ? AND to_entity_id = ? AND relation_type = ?",
		fromEntityID, toEntityID, relationType,
	)
	if err != nil {
		return err
	}
	if existing != nil {
		db.auditRelation("relation_delete", existing)
	}
	return nil
}

// GetEntityRelationByID retrieves a relation by its stable ID. Returns nil,
// nil if not found.
func (db *DB) GetEntityRelationByID(id string) (*EntityRelation, error) {
	row := db.conn.QueryRow(
		`SELECT id, from_entity_id, to_entity_id, relation_type, source, source_ref, verification, evidence, created_by, created_at, updated_at, valid_from, valid_until
			FROM entity_relations WHERE id = ?`,
		id,
	)
	var r EntityRelation
	err := row.Scan(&r.ID, &r.FromEntityID, &r.ToEntityID, &r.RelationType, &r.Source, &r.SourceRef, &r.Verification, &r.Evidence, &r.CreatedBy, &r.CreatedAt, &r.UpdatedAt, &r.ValidFrom, &r.ValidUntil)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return &r, nil
}

// DeleteEntityRelationByID removes a relation by its stable ID.
func (db *DB) DeleteEntityRelationByID(id string) error {
	existing, err := db.GetEntityRelationByID(id)
	if err != nil {
		return err
	}
	_, err = db.conn.Exec("DELETE FROM entity_relations WHERE id = ?", id)
	if err != nil {
		return err
	}
	if existing != nil {
		db.auditRelation("relation_delete", existing)
	}
	return nil
}

func (db *DB) getRelationByTriple(fromEntityID, toEntityID, relationType string) (*EntityRelation, error) {
	row := db.conn.QueryRow(
		`SELECT id, from_entity_id, to_entity_id, relation_type, source, source_ref, verification, evidence, created_by, created_at, updated_at, valid_from, valid_until
			FROM entity_relations WHERE from_entity_id = ? AND to_entity_id = ? AND relation_type = ?`,
		fromEntityID, toEntityID, relationType,
	)
	var r EntityRelation
	err := row.Scan(&r.ID, &r.FromEntityID, &r.ToEntityID, &r.RelationType, &r.Source, &r.SourceRef, &r.Verification, &r.Evidence, &r.CreatedBy, &r.CreatedAt, &r.UpdatedAt, &r.ValidFrom, &r.ValidUntil)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return &r, nil
}

// OutgoingRelations returns every relation where entityID is the source.
func (db *DB) OutgoingRelations(entityID string) ([]*EntityRelation, error) {
	return db.queryRelations(
		`SELECT id, from_entity_id, to_entity_id, relation_type, source, source_ref, verification, evidence, created_by, created_at, updated_at, valid_from, valid_until
			FROM entity_relations WHERE from_entity_id = ? ORDER BY relation_type, to_entity_id`,
		entityID,
	)
}

// IncomingRelations returns every relation where entityID is the target.
func (db *DB) IncomingRelations(entityID string) ([]*EntityRelation, error) {
	return db.queryRelations(
		`SELECT id, from_entity_id, to_entity_id, relation_type, source, source_ref, verification, evidence, created_by, created_at, updated_at, valid_from, valid_until
			FROM entity_relations WHERE to_entity_id = ? ORDER BY relation_type, from_entity_id`,
		entityID,
	)
}

// RelationsForEntity returns every relation touching entityID, in either
// direction (outgoing first, then incoming).
func (db *DB) RelationsForEntity(entityID string) ([]*EntityRelation, error) {
	out, err := db.OutgoingRelations(entityID)
	if err != nil {
		return nil, err
	}
	in, err := db.IncomingRelations(entityID)
	if err != nil {
		return nil, err
	}
	return append(out, in...), nil
}

// GraphNeighbors performs a breadth-first traversal of entity_relations
// starting at entityID out to depth hops (1 means direct relations only).
// It returns every entity reached (including the starting entity) and every
// distinct edge traversed. Cycles are handled: a node or edge is never
// visited/returned twice regardless of how many paths reach it.
func (db *DB) GraphNeighbors(entityID string, depth int) ([]*Entity, []*EntityRelation, error) {
	if depth < 1 {
		depth = 1
	}
	if depth > MaxGraphDepth {
		return nil, nil, fmt.Errorf("depth %d exceeds maximum allowed depth %d", depth, MaxGraphDepth)
	}

	visited := map[string]bool{entityID: true}
	edgeSeen := map[string]bool{}
	var edges []*EntityRelation

	frontier := []string{entityID}
	for hop := 0; hop < depth && len(frontier) > 0; hop++ {
		var next []string
		for _, id := range frontier {
			rels, err := db.RelationsForEntity(id)
			if err != nil {
				return nil, nil, err
			}
			for _, r := range rels {
				key := r.FromEntityID + "\x00" + r.ToEntityID + "\x00" + r.RelationType
				if !edgeSeen[key] {
					edgeSeen[key] = true
					edges = append(edges, r)
				}
				other := r.ToEntityID
				if other == id {
					other = r.FromEntityID
				}
				if !visited[other] {
					visited[other] = true
					next = append(next, other)
				}
			}
		}
		frontier = next
	}

	nodes := make([]*Entity, 0, len(visited))
	for id := range visited {
		e, err := db.GetEntityByID(id)
		if err != nil {
			return nil, nil, err
		}
		if e != nil {
			nodes = append(nodes, e)
		}
	}

	return nodes, edges, nil
}

func (db *DB) queryRelations(query, entityID string) ([]*EntityRelation, error) {
	rows, err := db.conn.Query(query, entityID)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()

	var relations []*EntityRelation
	for rows.Next() {
		var r EntityRelation
		if err := rows.Scan(&r.ID, &r.FromEntityID, &r.ToEntityID, &r.RelationType, &r.Source, &r.SourceRef, &r.Verification, &r.Evidence, &r.CreatedBy, &r.CreatedAt, &r.UpdatedAt, &r.ValidFrom, &r.ValidUntil); err != nil {
			return nil, err
		}
		relations = append(relations, &r)
	}
	return relations, rows.Err()
}

// ParseRelationDate parses a date string accepting RFC3339 or YYYY-MM-DD
// format. The parsed time is returned in UTC.
func ParseRelationDate(s string) (time.Time, error) {
	s = strings.TrimSpace(s)
	if s == "" {
		return time.Time{}, fmt.Errorf("empty date string")
	}
	// Try RFC3339 first.
	if t, err := time.Parse(time.RFC3339, s); err == nil {
		return t.UTC(), nil
	}
	// Try YYYY-MM-DD.
	if t, err := time.Parse("2006-01-02", s); err == nil {
		return t.UTC(), nil
	}
	return time.Time{}, fmt.Errorf("invalid date %q: must be RFC3339 or YYYY-MM-DD", s)
}

// RelationsForEntityAsOf returns every relation touching entityID that was
// valid at the given point in time. A relation is valid at asOf when:
//
//	(valid_from IS NULL OR valid_from <= asOf) AND
//	(valid_until IS NULL OR valid_until > asOf)
//
// Relations without any temporal fields (NULL valid_from AND valid_until)
// are always considered valid (open-ended).
func (db *DB) RelationsForEntityAsOf(entityID string, asOf time.Time) ([]*EntityRelation, error) {
	out, err := db.queryRelationsAsOf(
		`SELECT id, from_entity_id, to_entity_id, relation_type, source, source_ref, verification, evidence, created_by, created_at, updated_at, valid_from, valid_until
			FROM entity_relations WHERE from_entity_id = ?
			AND (valid_from IS NULL OR valid_from <= ?)
			AND (valid_until IS NULL OR valid_until > ?)
			ORDER BY relation_type, to_entity_id`,
		entityID, asOf,
	)
	if err != nil {
		return nil, err
	}
	in, err := db.queryRelationsAsOf(
		`SELECT id, from_entity_id, to_entity_id, relation_type, source, source_ref, verification, evidence, created_by, created_at, updated_at, valid_from, valid_until
			FROM entity_relations WHERE to_entity_id = ?
			AND (valid_from IS NULL OR valid_from <= ?)
			AND (valid_until IS NULL OR valid_until > ?)
			ORDER BY relation_type, from_entity_id`,
		entityID, asOf,
	)
	if err != nil {
		return nil, err
	}
	return append(out, in...), nil
}

func (db *DB) queryRelationsAsOf(query, entityID string, asOf time.Time) ([]*EntityRelation, error) {
	rows, err := db.conn.Query(query, entityID, asOf, asOf)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()

	var relations []*EntityRelation
	for rows.Next() {
		var r EntityRelation
		if err := rows.Scan(&r.ID, &r.FromEntityID, &r.ToEntityID, &r.RelationType, &r.Source, &r.SourceRef, &r.Verification, &r.Evidence, &r.CreatedBy, &r.CreatedAt, &r.UpdatedAt, &r.ValidFrom, &r.ValidUntil); err != nil {
			return nil, err
		}
		relations = append(relations, &r)
	}
	return relations, rows.Err()
}

// GraphNeighborsAsOf performs a breadth-first traversal like GraphNeighbors,
// but only traverses relations valid at the given point in time. If asOf is
// nil, the current time is used (equivalent to GraphNeighbors).
func (db *DB) GraphNeighborsAsOf(entityID string, depth int, asOf *time.Time) ([]*Entity, []*EntityRelation, error) {
	if depth < 1 {
		depth = 1
	}
	if depth > MaxGraphDepth {
		return nil, nil, fmt.Errorf("depth %d exceeds maximum allowed depth %d", depth, MaxGraphDepth)
	}

	now := time.Now().UTC()
	if asOf == nil {
		asOf = &now
	}

	visited := map[string]bool{entityID: true}
	edgeSeen := map[string]bool{}
	var edges []*EntityRelation

	frontier := []string{entityID}
	for hop := 0; hop < depth && len(frontier) > 0; hop++ {
		var next []string
		for _, id := range frontier {
			rels, err := db.RelationsForEntityAsOf(id, *asOf)
			if err != nil {
				return nil, nil, err
			}
			for _, r := range rels {
				key := r.FromEntityID + "\x00" + r.ToEntityID + "\x00" + r.RelationType
				if !edgeSeen[key] {
					edgeSeen[key] = true
					edges = append(edges, r)
				}
				other := r.ToEntityID
				if other == id {
					other = r.FromEntityID
				}
				if !visited[other] {
					visited[other] = true
					next = append(next, other)
				}
			}
		}
		frontier = next
	}

	nodes := make([]*Entity, 0, len(visited))
	for id := range visited {
		e, err := db.GetEntityByID(id)
		if err != nil {
			return nil, nil, err
		}
		if e != nil {
			nodes = append(nodes, e)
		}
	}

	return nodes, edges, nil
}

// auditRelation writes exactly one audit event for a relation mutation.
// detail carries the full triple plus any supplied provenance so the edge
// is reconstructible after the fact. Audit failures never fail the
// mutation; the identical-retry path in SaveEntityRelationProvenance
// returns before this helper is called, so no duplicate event is emitted.
func (db *DB) auditRelation(action string, r *EntityRelation) {
	detail, _ := json.Marshal(map[string]string{
		"from":         r.FromEntityID,
		"to":           r.ToEntityID,
		"relation":     r.RelationType,
		"source":       r.Source,
		"source_ref":   r.SourceRef,
		"verification": r.Verification,
	})
	_ = db.LogTargetAudit(action, TargetRelation, r.ID, "", "", r.CreatedBy, string(detail))
}
