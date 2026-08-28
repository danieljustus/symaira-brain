package activity

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
	"github.com/danieljustus/symaira-corekit/evidencekit"
)

// Store persists activity rows through the existing memory DB connection.
type Store struct {
	db   *db.DB
	opts Options
}

// NewStore creates an activity store backed by database. It never opens a
// separate database and enables SQLite secure deletion on the shared handle.
func NewStore(database *db.DB, options ...Options) (*Store, error) {
	if database == nil {
		return nil, fmt.Errorf("activity store requires a database")
	}
	opts := Options{}
	if len(options) > 0 {
		opts = options[0]
	}
	opts = normalizeOptions(opts)
	if _, err := database.Conn().Exec("PRAGMA secure_delete = ON"); err != nil {
		return nil, fmt.Errorf("enable secure activity deletion: %w", err)
	}
	return &Store{db: database, opts: opts}, nil
}

// NewStoreFromConfig creates a Store using the existing memory retention
// configuration. The database argument must have been opened by db.Open.
func NewStoreFromConfig(database *db.DB, cfg *config.Config) (*Store, error) {
	if cfg == nil {
		cfg = config.Defaults()
	}
	return NewStore(database, NewOptionsFromConfig(cfg.Retention.ActivitySegmentTTL, cfg.Retention.ActivityEpisodeTTL))
}

// Database returns the shared database used by the store.
func (s *Store) Database() *db.DB { return s.db }

// SaveSegment redacts and upserts one fixed-window activity segment.
func (s *Store) SaveSegment(segment Segment) error {
	segment = s.prepareSegment(segment)
	if err := segment.validate(); err != nil {
		return err
	}
	return s.saveSegmentExec(s.db.Conn(), segment)
}

// PutSegment is an alias for SaveSegment for provider adapters.
func (s *Store) PutSegment(segment Segment) error { return s.SaveSegment(segment) }

func (s *Store) prepareSegment(segment Segment) Segment {
	segment.Source = security.Redact(strings.TrimSpace(segment.Source))
	segment.RedactedSummary = security.Redact(segment.RedactedSummary)
	segment.RawRef = security.Redact(segment.RawRef)
	segment.Applications = redactStrings(segment.Applications)
	segment.PriorSegmentIDs = uniqueSorted(segment.PriorSegmentIDs)
	if segment.ID == "" && !segment.StartedAt.IsZero() && !segment.EndedAt.IsZero() {
		segment.ID = segmentID(segment.Source, segment.Granularity, segment.StartedAt, segment.EndedAt)
	}
	if segment.ExpiresAt.IsZero() {
		segment.ExpiresAt = segment.EndedAt.Add(s.opts.SegmentTTL)
	}
	return segment
}

func (s *Store) saveSegmentExec(execer db.SQLExecer, segment Segment) error {
	applications, err := marshalStrings(segment.Applications)
	if err != nil {
		return fmt.Errorf("encode activity applications: %w", err)
	}
	prior, err := marshalStrings(segment.PriorSegmentIDs)
	if err != nil {
		return fmt.Errorf("encode prior activity segments: %w", err)
	}
	_, err = execer.Exec(`INSERT INTO activity_segments
		(id, source, granularity, started_at, ended_at, applications, redacted_summary, raw_ref, prior_segment_ids, superseded_by, expires_at)
		VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
		ON CONFLICT(id) DO UPDATE SET
		 source=excluded.source, granularity=excluded.granularity,
		 started_at=excluded.started_at, ended_at=excluded.ended_at,
		 applications=excluded.applications, redacted_summary=excluded.redacted_summary,
		 raw_ref=excluded.raw_ref, prior_segment_ids=excluded.prior_segment_ids,
		 superseded_by=excluded.superseded_by, expires_at=excluded.expires_at`,
		segment.ID, segment.Source, segment.Granularity, segment.StartedAt.UTC(), segment.EndedAt.UTC(),
		applications, segment.RedactedSummary, segment.RawRef, prior, segment.SupersededBy, segment.ExpiresAt.UTC())
	return err
}

// GetSegment returns one segment without changing retention state.
func (s *Store) GetSegment(id string) (*Segment, error) {
	return scanSegmentRow(s.db.Conn().QueryRow(`SELECT id, source, granularity,
		started_at, ended_at, applications, redacted_summary, raw_ref,
		prior_segment_ids, superseded_by, expires_at
		FROM activity_segments WHERE id = ?`, id))
}

// ListSegments returns non-expired segments for an overlapping time range.
// Fine-grained rows covered by a 6h rollup are omitted by default.
func (s *Store) ListSegments(start, end time.Time) ([]Segment, error) {
	return s.listSegments(start, end, false)
}

// ListAllSegments returns both granularities, including rows superseded by a
// coarser rollup. It is intended for audit and deletion workflows.
func (s *Store) ListAllSegments(start, end time.Time) ([]Segment, error) {
	return s.listSegments(start, end, true)
}

func (s *Store) listSegments(start, end time.Time, includeSuperseded bool) ([]Segment, error) {
	query := `SELECT id, source, granularity, started_at, ended_at, applications,
		redacted_summary, raw_ref, prior_segment_ids, superseded_by, expires_at
		FROM activity_segments WHERE ended_at > ? AND started_at < ? AND expires_at > ?`
	if !includeSuperseded {
		query += ` AND (granularity != '10min' OR superseded_by = '')`
	}
	query += ` ORDER BY started_at ASC, id ASC`
	rows, err := s.db.Conn().Query(query, start.UTC(), end.UTC(), s.now())
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var segments []Segment
	for rows.Next() {
		segment, err := scanSegment(rows)
		if err != nil {
			return nil, err
		}
		segments = append(segments, *segment)
	}
	return segments, rows.Err()
}

// RollUp6Hour creates or replaces the deterministic 6h segment for source
// and start. It records every consulted 10min segment and marks those rows as
// superseded. Re-running it is idempotent.
func (s *Store) RollUp6Hour(source string, start time.Time) (*Segment, error) {
	source = security.Redact(strings.TrimSpace(source))
	start = start.UTC()
	end := start.Add(6 * time.Hour)
	rows, err := s.db.Conn().Query(`SELECT id, source, granularity, started_at,
		ended_at, applications, redacted_summary, raw_ref, prior_segment_ids,
		superseded_by, expires_at FROM activity_segments
		WHERE source = ? AND granularity = '10min' AND started_at >= ? AND ended_at <= ?
		AND expires_at > ? ORDER BY started_at ASC, id ASC`, source, start, end, s.now())
	if err != nil {
		return nil, err
	}
	var fine []Segment
	for rows.Next() {
		segment, scanErr := scanSegment(rows)
		if scanErr != nil {
			_ = rows.Close()
			return nil, scanErr
		}
		fine = append(fine, *segment)
	}
	if err := rows.Err(); err != nil {
		_ = rows.Close()
		return nil, err
	}
	_ = rows.Close()
	if len(fine) == 0 {
		return nil, fmt.Errorf("no active 10min segments in %s activity window", start.Format(time.RFC3339))
	}

	var summaries []string
	var applications []string
	var prior []string
	for _, segment := range fine {
		summaries = append(summaries, segment.RedactedSummary)
		applications = append(applications, segment.Applications...)
		prior = append(prior, segment.ID)
		prior = append(prior, segment.PriorSegmentIDs...)
	}
	rollup := Segment{
		ID:              segmentID(source, Granularity6Hour, start, end),
		Source:          source,
		Granularity:     Granularity6Hour,
		StartedAt:       start,
		EndedAt:         end,
		Applications:    uniqueSorted(applications),
		RedactedSummary: security.Redact(strings.Join(summaries, "\n")),
		RawRef:          "rollup:" + source + ":" + start.Format(time.RFC3339),
		PriorSegmentIDs: uniqueSorted(prior),
		ExpiresAt:       end.Add(s.opts.SegmentTTL),
	}
	rollup = s.prepareSegment(rollup)
	if err := rollup.validate(); err != nil {
		return nil, err
	}
	tx, err := s.db.BeginTransaction()
	if err != nil {
		return nil, err
	}
	if err := s.saveSegmentExec(tx, rollup); err != nil {
		_ = tx.Rollback()
		return nil, fmt.Errorf("save 6h activity rollup: %w", err)
	}
	for _, segment := range fine {
		if _, err := tx.Exec(`UPDATE activity_segments SET superseded_by = ? WHERE id = ?`, rollup.ID, segment.ID); err != nil {
			_ = tx.Rollback()
			return nil, fmt.Errorf("mark activity segment %s superseded: %w", segment.ID, err)
		}
	}
	if err := tx.Commit(); err != nil {
		return nil, err
	}
	return &rollup, nil
}

// Rollup is the conventional spelling alias for RollUp6Hour.
func (s *Store) Rollup(source string, start time.Time) (*Segment, error) {
	return s.RollUp6Hour(source, start)
}

// SaveEpisode redacts and upserts one activity episode.
func (s *Store) SaveEpisode(episode Episode) error {
	episode = s.prepareEpisode(episode)
	if err := episode.validate(); err != nil {
		return err
	}
	sources, err := marshalStrings(episode.SegmentIDs)
	if err != nil {
		return err
	}
	citations, err := marshalStrings(redactStrings(episode.Citations))
	if err != nil {
		return err
	}
	_, err = s.db.Conn().Exec(`INSERT INTO activity_episodes
		(id, title, scope, started_at, ended_at, confidence, sources, citations, expires_at)
		VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
		ON CONFLICT(id) DO UPDATE SET title=excluded.title, scope=excluded.scope,
		started_at=excluded.started_at, ended_at=excluded.ended_at,
		confidence=excluded.confidence, sources=excluded.sources,
		citations=excluded.citations, expires_at=excluded.expires_at`,
		episode.ID, episode.Title, episode.Scope, episode.StartedAt.UTC(), episode.EndedAt.UTC(),
		episode.Confidence, sources, citations, episode.ExpiresAt.UTC())
	return err
}

// PutEpisode is an alias for SaveEpisode.
func (s *Store) PutEpisode(episode Episode) error { return s.SaveEpisode(episode) }

func (s *Store) prepareEpisode(episode Episode) Episode {
	episode.Title = security.Redact(strings.TrimSpace(episode.Title))
	episode.Scope = security.Redact(strings.TrimSpace(episode.Scope))
	episode.SegmentIDs = uniqueSorted(episode.SegmentIDs)
	episode.Citations = uniqueSorted(redactStrings(episode.Citations))
	if episode.ID == "" && !episode.StartedAt.IsZero() && !episode.EndedAt.IsZero() {
		episode.ID = episodeID(episode.Scope, episode.StartedAt, episode.EndedAt)
	}
	if episode.ExpiresAt.IsZero() {
		episode.ExpiresAt = episode.EndedAt.Add(s.opts.EpisodeTTL)
	}
	return episode
}

// GetEpisode returns one episode, or (nil, nil) if it does not exist.
func (s *Store) GetEpisode(id string) (*Episode, error) {
	return scanEpisodeRow(s.db.Conn().QueryRow(`SELECT id, title, scope, started_at,
		ended_at, confidence, sources, citations, expires_at
		FROM activity_episodes WHERE id = ?`, id))
}

// ListEpisodes returns non-expired episodes overlapping a time range.
func (s *Store) ListEpisodes(start, end time.Time) ([]Episode, error) {
	rows, err := s.db.Conn().Query(`SELECT id, title, scope, started_at, ended_at,
		confidence, sources, citations, expires_at FROM activity_episodes
		WHERE ended_at > ? AND started_at < ? AND expires_at > ?
		ORDER BY started_at ASC, id ASC`, start.UTC(), end.UTC(), s.now())
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var episodes []Episode
	for rows.Next() {
		episode, err := scanEpisode(rows)
		if err != nil {
			return nil, err
		}
		episodes = append(episodes, *episode)
	}
	return episodes, rows.Err()
}

// StageEpisode creates a staged memory candidate for an episode and writes
// grounded evidence for every source segment in the same transaction. It
// never approves the candidate; approval remains the existing explicit
// memory_promote boundary. A lone 10min observation is refused to prevent
// turning one window into a durable profile fact.
func (s *Store) StageEpisode(episodeID string) (*db.Memory, error) {
	episode, err := s.GetEpisode(episodeID)
	if err != nil || episode == nil {
		if err == nil {
			err = fmt.Errorf("activity episode not found: %s", episodeID)
		}
		return nil, err
	}
	segments := make([]Segment, 0, len(episode.SegmentIDs))
	for _, id := range episode.SegmentIDs {
		segment, err := s.GetSegment(id)
		if err != nil {
			return nil, err
		}
		if segment == nil {
			return nil, fmt.Errorf("activity segment not found: %s", id)
		}
		segments = append(segments, *segment)
	}
	if len(segments) == 1 && segments[0].Granularity == Granularity10Min {
		return nil, fmt.Errorf("single 10min activity segment cannot become a durable candidate")
	}
	memoryID := stableID("memory", "activity", episode.ID)
	metadata := map[string]string{
		"source":              "activity",
		"activity_source":     "true",
		"activity_episode_id": episode.ID,
		"sync_exclude":        "true",
		"promotion_boundary":  "memory_candidates",
	}
	memory := &db.Memory{
		ID:                  memoryID,
		Content:             security.Redact(episode.Title),
		Scope:               security.Redact(episode.Scope),
		Metadata:            metadata,
		CreatedAt:           episode.StartedAt,
		UpdatedAt:           s.now(),
		CreatedBy:           "activity",
		UpdatedBy:           "activity",
		CreatedSession:      episode.ID,
		UpdatedSession:      episode.ID,
		ConsolidationStatus: "raw",
		ReviewStatus:        db.ReviewStaged,
		Kind:                db.KindReference,
		Importance:          episode.Confidence,
	}
	if memory.Scope == "" {
		memory.Scope = "activity"
	}
	if memory.Importance == 0 {
		memory.Importance = 0.5
	}
	tx, err := s.db.BeginTransaction()
	if err != nil {
		return nil, err
	}
	if err := s.db.SaveMemoryTx(tx, memory); err != nil {
		_ = tx.Rollback()
		return nil, fmt.Errorf("stage activity episode memory: %w", err)
	}
	for _, segment := range segments {
		ext := evidencekit.Extraction{
			Source:          evidencekit.SourceRef{ID: segment.ID, Kind: "activity_segment"},
			Text:            segment.RedactedSummary,
			EvidenceText:    segment.RedactedSummary,
			Span:            evidencekit.Span{Start: 0, End: len(segment.RedactedSummary)},
			AlignmentStatus: evidencekit.AlignmentExact,
		}
		if err := s.db.SaveMemoryEvidenceTx(tx, memory.ID, []evidencekit.Extraction{ext}); err != nil {
			_ = tx.Rollback()
			return nil, fmt.Errorf("stage activity evidence for %s: %w", segment.ID, err)
		}
	}
	if err := tx.Commit(); err != nil {
		return nil, err
	}
	return memory, nil
}

// PromoteEpisodeToCandidate names the boundary explicitly for callers.
func (s *Store) PromoteEpisodeToCandidate(episodeID string) (*db.Memory, error) {
	return s.StageEpisode(episodeID)
}

// PromoteEpisode is retained as a friendly alias; it still only stages a
// candidate and cannot bypass memory promotion review.
func (s *Store) PromoteEpisode(episodeID string) (*db.Memory, error) {
	return s.StageEpisode(episodeID)
}

func (s *Store) now() time.Time { return s.opts.Now().UTC() }

func scanSegment(rows interface{ Scan(...any) error }) (*Segment, error) {
	var segment Segment
	var applications, prior string
	if err := rows.Scan(&segment.ID, &segment.Source, &segment.Granularity,
		&segment.StartedAt, &segment.EndedAt, &applications,
		&segment.RedactedSummary, &segment.RawRef, &prior,
		&segment.SupersededBy, &segment.ExpiresAt); err != nil {
		if err == sql.ErrNoRows {
			return nil, nil
		}
		return nil, err
	}
	var err error
	segment.Applications, err = unmarshalStrings(applications)
	if err != nil {
		return nil, fmt.Errorf("decode segment applications: %w", err)
	}
	segment.PriorSegmentIDs, err = unmarshalStrings(prior)
	if err != nil {
		return nil, fmt.Errorf("decode prior segment references: %w", err)
	}
	return &segment, nil
}

func scanSegmentRows(rows *sql.Rows) (*Segment, error) { return scanSegment(rows) }

func scanEpisode(rows interface{ Scan(...any) error }) (*Episode, error) {
	var episode Episode
	var sources, citations string
	if err := rows.Scan(&episode.ID, &episode.Title, &episode.Scope,
		&episode.StartedAt, &episode.EndedAt, &episode.Confidence,
		&sources, &citations, &episode.ExpiresAt); err != nil {
		if err == sql.ErrNoRows {
			return nil, nil
		}
		return nil, err
	}
	var err error
	episode.SegmentIDs, err = unmarshalStrings(sources)
	if err != nil {
		return nil, fmt.Errorf("decode episode sources: %w", err)
	}
	episode.Citations, err = unmarshalStrings(citations)
	if err != nil {
		return nil, fmt.Errorf("decode episode citations: %w", err)
	}
	return &episode, nil
}

func scanEpisodeRows(rows *sql.Rows) (*Episode, error) { return scanEpisode(rows) }

func scanSegmentRow(row *sql.Row) (*Segment, error) { return scanSegment(row) }
func scanEpisodeRow(row *sql.Row) (*Episode, error) { return scanEpisode(row) }

func redactStrings(values []string) []string {
	if values == nil {
		return nil
	}
	redacted := make([]string, len(values))
	for i, value := range values {
		redacted[i] = security.Redact(value)
	}
	return redacted
}

// JSONShapeCheck is a small diagnostic helper used by fixture tests and
// adapters. It confirms that a segment contains only references and redacted
// fields; RawPayload cannot be supplied because Segment has no raw payload.
func JSONShapeCheck(segment Segment) error {
	encoded, err := json.Marshal(segment)
	if err != nil {
		return err
	}
	if strings.Contains(string(encoded), "raw_payload") {
		return fmt.Errorf("raw payload field must not be serialized")
	}
	return nil
}

// DatabaseFileExists reports whether the shared database file is present.
// It is intentionally informational; storage remains owned by db.DB.
func (s *Store) DatabaseFileExists() bool {
	path := s.db.Path()
	if path == "" {
		return false
	}
	_, err := os.Stat(path)
	return err == nil
}
