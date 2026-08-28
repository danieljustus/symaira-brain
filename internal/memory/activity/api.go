package activity

import (
	"database/sql"
	"fmt"
	"strings"
	"time"
	"unicode/utf8"
)

// Read limits are deliberately conservative. The read API rejects requests
// that do not carry explicit bounds; these constants are hard ceilings even
// when a caller asks for a larger page or token budget.
const (
	MaxResults     = 50
	MaxTokens      = 4000
	MaxQueryLength = 512
	MaxRange       = 7 * 24 * time.Hour
)

// SearchOptions describes one bounded activity query. From and To are required
// and define a half-open window. Query matches redacted summaries, sources,
// application identifiers, episode titles, and scopes.
type SearchOptions struct {
	Query           string
	Source          string
	From            time.Time
	To              time.Time
	Limit           int
	MaxTokens       int
	IncludeEpisodes bool
}

// ReadItem is the safe, consumer-facing activity shape. It contains only
// redacted summaries and provenance pointers; raw observations are never part
// of this type or returned by the API.
type ReadItem struct {
	ID           string     `json:"id"`
	Kind         string     `json:"kind"`
	Source       string     `json:"source,omitempty"`
	Granularity  string     `json:"granularity,omitempty"`
	Title        string     `json:"title,omitempty"`
	Scope        string     `json:"scope,omitempty"`
	StartedAt    time.Time  `json:"started_at"`
	EndedAt      time.Time  `json:"ended_at"`
	Confidence   float64    `json:"confidence,omitempty"`
	Applications []string   `json:"applications,omitempty"`
	Summary      string     `json:"summary"`
	Provenance   Provenance `json:"provenance"`
	Tokens       int        `json:"tokens"`
}

// Provenance is a redacted chain of opaque pointers. Reference values identify
// the owner-side source but do not contain event payloads.
type Provenance struct {
	Source          string   `json:"source,omitempty"`
	Reference       string   `json:"reference,omitempty"`
	PriorSegmentIDs []string `json:"prior_segment_ids,omitempty"`
	DerivedFrom     []string `json:"derived_from,omitempty"`
	Citations       []string `json:"citations,omitempty"`
}

// SearchPage is the bounded activity search result.
type SearchPage struct {
	Results    []ReadItem `json:"results"`
	Truncated  bool       `json:"truncated"`
	UsedTokens int        `json:"used_tokens"`
	MaxTokens  int        `json:"max_tokens"`
}

// Status is a bounded operational summary of the activity store.
type Status struct {
	ActiveSegments int        `json:"active_segments"`
	ActiveEpisodes int        `json:"active_episodes"`
	SegmentTTL     string     `json:"segment_ttl"`
	EpisodeTTL     string     `json:"episode_ttl"`
	Earliest       *time.Time `json:"earliest,omitempty"`
	Latest         *time.Time `json:"latest,omitempty"`
}

// ValidateSearchOptions rejects unbounded or excessively broad activity
// queries before any database work occurs.
func ValidateSearchOptions(opts SearchOptions) error {
	if strings.TrimSpace(opts.Query) == "" {
		return fmt.Errorf("activity query is required")
	}
	if utf8.RuneCountInString(opts.Query) > MaxQueryLength {
		return fmt.Errorf("activity query exceeds %d characters", MaxQueryLength)
	}
	if opts.From.IsZero() || opts.To.IsZero() || !opts.To.After(opts.From) {
		return fmt.Errorf("activity query requires an increasing from/to window")
	}
	if opts.To.Sub(opts.From) > MaxRange {
		return fmt.Errorf("activity query window exceeds %s", MaxRange)
	}
	if opts.Limit < 1 || opts.Limit > MaxResults {
		return fmt.Errorf("activity query limit must be between 1 and %d", MaxResults)
	}
	if opts.MaxTokens < 1 || opts.MaxTokens > MaxTokens {
		return fmt.Errorf("activity query max_tokens must be between 1 and %d", MaxTokens)
	}
	return nil
}

// Search returns only bounded, non-expired redacted activity items.
func (s *Store) Search(opts SearchOptions) (SearchPage, error) {
	if err := ValidateSearchOptions(opts); err != nil {
		return SearchPage{}, err
	}
	query := strings.ToLower(strings.TrimSpace(opts.Query))
	segments, err := s.ListSegments(opts.From, opts.To)
	if err != nil {
		return SearchPage{}, fmt.Errorf("search activity segments: %w", err)
	}
	items := make([]ReadItem, 0, len(segments))
	for _, segment := range segments {
		if opts.Source != "" && !strings.EqualFold(opts.Source, segment.Source) {
			continue
		}
		fields := append([]string{segment.Source, segment.RedactedSummary}, segment.Applications...)
		if !containsActivityText(query, fields...) {
			continue
		}
		items = append(items, segmentItem(segment))
	}
	if opts.IncludeEpisodes {
		episodes, err := s.ListEpisodes(opts.From, opts.To)
		if err != nil {
			return SearchPage{}, fmt.Errorf("search activity episodes: %w", err)
		}
		for _, episode := range episodes {
			if !containsActivityText(query, episode.Title, episode.Scope) {
				continue
			}
			items = append(items, episodeItem(episode))
		}
	}
	// The store lists each kind deterministically. Keep the API deterministic
	// when episodes and segments are combined as well.
	sortReadItems(items)
	page := SearchPage{Results: make([]ReadItem, 0, minInt(len(items), opts.Limit)), MaxTokens: opts.MaxTokens}
	for _, item := range items {
		if len(page.Results) >= opts.Limit {
			page.Truncated = true
			break
		}
		remaining := opts.MaxTokens - page.UsedTokens
		if remaining <= 0 {
			page.Truncated = true
			break
		}
		item.Summary = fitSummary(item.Summary, remaining)
		item.Tokens = estimateActivityTokens(item.Summary)
		if item.Tokens > remaining {
			page.Truncated = true
			break
		}
		page.Results = append(page.Results, item)
		page.UsedTokens += item.Tokens
	}
	if len(page.Results) < len(items) {
		page.Truncated = true
	}
	return page, nil
}

func segmentItem(segment Segment) ReadItem {
	return ReadItem{
		ID: segment.ID, Kind: "segment", Source: segment.Source,
		Granularity: segment.Granularity, StartedAt: segment.StartedAt,
		EndedAt: segment.EndedAt, Applications: append([]string(nil), segment.Applications...),
		Summary: segment.RedactedSummary,
		Provenance: Provenance{Source: segment.Source, Reference: segment.RawRef,
			PriorSegmentIDs: append([]string(nil), segment.PriorSegmentIDs...),
			DerivedFrom:     nonEmptyRefs(segment.SupersededBy)},
	}
}

func episodeItem(episode Episode) ReadItem {
	return ReadItem{
		ID: episode.ID, Kind: "episode", Title: episode.Title, Scope: episode.Scope,
		StartedAt: episode.StartedAt, EndedAt: episode.EndedAt,
		Confidence: episode.Confidence, Summary: episode.Title,
		Provenance: Provenance{DerivedFrom: append([]string(nil), episode.SegmentIDs...),
			Citations: append([]string(nil), episode.Citations...)},
	}
}

func containsActivityText(query string, fields ...string) bool {
	for _, field := range fields {
		if strings.Contains(strings.ToLower(field), query) {
			return true
		}
	}
	return false
}

func fitSummary(summary string, maxTokens int) string {
	maxRunes := maxTokens * 4
	if maxRunes < 1 {
		return ""
	}
	runes := []rune(summary)
	if len(runes) <= maxRunes {
		return summary
	}
	return string(runes[:maxRunes])
}

func estimateActivityTokens(text string) int {
	if text == "" {
		return 0
	}
	return utf8.RuneCountInString(text)/4 + 1
}

func nonEmptyRefs(value string) []string {
	if value == "" {
		return nil
	}
	return []string{value}
}

func minInt(a, b int) int {
	if a < b {
		return a
	}
	return b
}

func sortReadItems(items []ReadItem) {
	for i := 1; i < len(items); i++ {
		for j := i; j > 0 && readItemBefore(items[j], items[j-1]); j-- {
			items[j], items[j-1] = items[j-1], items[j]
		}
	}
}

func readItemBefore(a, b ReadItem) bool {
	if !a.StartedAt.Equal(b.StartedAt) {
		return a.StartedAt.Before(b.StartedAt)
	}
	return a.ID < b.ID
}

// GetReadItem returns a safe read item for a segment or episode ID.
func (s *Store) GetReadItem(id string) (*ReadItem, error) {
	if strings.TrimSpace(id) == "" {
		return nil, fmt.Errorf("activity id is required")
	}
	if segment, err := s.GetSegment(id); err != nil {
		return nil, err
	} else if segment != nil && segment.ExpiresAt.After(s.now()) {
		item := segmentItem(*segment)
		return &item, nil
	}
	if episode, err := s.GetEpisode(id); err != nil {
		return nil, err
	} else if episode != nil && episode.ExpiresAt.After(s.now()) {
		item := episodeItem(*episode)
		return &item, nil
	}
	return nil, nil
}

// Status returns counts and the retained time span without exposing activity
// content or raw event rows.
func (s *Store) Status() (Status, error) {
	status := Status{SegmentTTL: s.opts.SegmentTTL.String(), EpisodeTTL: s.opts.EpisodeTTL.String()}
	now := s.now()
	if err := s.db.Conn().QueryRow("SELECT COUNT(*) FROM activity_segments WHERE expires_at > ?", now).Scan(&status.ActiveSegments); err != nil {
		return Status{}, err
	}
	if err := s.db.Conn().QueryRow("SELECT COUNT(*) FROM activity_episodes WHERE expires_at > ?", now).Scan(&status.ActiveEpisodes); err != nil {
		return Status{}, err
	}
	var earliest, latest sql.NullString
	if err := s.db.Conn().QueryRow("SELECT MIN(started_at), MAX(ended_at) FROM activity_segments WHERE expires_at > ?", now).Scan(&earliest, &latest); err != nil {
		return Status{}, err
	}
	if earliest.Valid {
		earliestTime, err := parseActivityTime(earliest.String)
		if err != nil {
			return Status{}, fmt.Errorf("parse earliest activity timestamp %q: %w", earliest.String, err)
		}
		latestTime, err := parseActivityTime(latest.String)
		if err != nil {
			return Status{}, fmt.Errorf("parse latest activity timestamp %q: %w", latest.String, err)
		}
		status.Earliest = &earliestTime
		status.Latest = &latestTime
	}
	return status, nil
}

func parseActivityTime(raw string) (time.Time, error) {
	layouts := []string{
		"2006-01-02 15:04:05.999999999 -0700 MST",
		"2006-01-02 15:04:05 -0700 MST",
		"2006-01-02 15:04:05-07:00",
		"2006-01-02 15:04:05",
		time.RFC3339Nano,
	}
	var lastErr error
	for _, layout := range layouts {
		parsed, err := time.Parse(layout, raw)
		if err == nil {
			return parsed, nil
		}
		lastErr = err
	}
	return time.Time{}, lastErr
}
