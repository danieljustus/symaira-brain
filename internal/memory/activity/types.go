// Package activity stores short-lived, provider-neutral computer activity
// observations separately from durable memories. It owns no capture code and
// never persists raw event payloads.
package activity

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"sort"
	"strings"
	"time"
)

const (
	// Granularity10Min is the fine-grained activity window.
	Granularity10Min = "10min"
	// Granularity6Hour is the coarse activity rollup window.
	Granularity6Hour  = "6h"
	defaultSegmentTTL = 48 * time.Hour
	defaultEpisodeTTL = 30 * 24 * time.Hour
)

// DefaultSegmentTTL is the default activity segment retention window.
func DefaultSegmentTTL() time.Duration { return defaultSegmentTTL }

// DefaultEpisodeTTL is the default activity episode retention window.
func DefaultEpisodeTTL() time.Duration { return defaultEpisodeTTL }

// Segment is a redacted, fixed-window activity observation. Raw event payloads
// are intentionally not represented: RawRef is only a reference owned by the
// provider that captured the observation.
type Segment struct {
	ID              string    `json:"id"`
	Source          string    `json:"source"`
	Granularity     string    `json:"granularity"`
	StartedAt       time.Time `json:"started_at"`
	EndedAt         time.Time `json:"ended_at"`
	Applications    []string  `json:"applications"`
	RedactedSummary string    `json:"redacted_summary"`
	RawRef          string    `json:"raw_ref,omitempty"`
	PriorSegmentIDs []string  `json:"prior_segment_ids,omitempty"`
	SupersededBy    string    `json:"superseded_by,omitempty"`
	ExpiresAt       time.Time `json:"expires_at"`
}

// Episode is a longer-lived activity rollup assembled from segment IDs. Its
// sources and citations are references, never raw event content.
type Episode struct {
	ID         string    `json:"id"`
	Title      string    `json:"title"`
	Scope      string    `json:"scope"`
	StartedAt  time.Time `json:"started_at"`
	EndedAt    time.Time `json:"ended_at"`
	Confidence float64   `json:"confidence"`
	SegmentIDs []string  `json:"sources"`
	Citations  []string  `json:"citations,omitempty"`
	ExpiresAt  time.Time `json:"expires_at"`
}

// Options configures a Store. DB must be an already-opened memory database;
// the activity store never opens a second SQLite database.
type Options struct {
	SegmentTTL time.Duration
	EpisodeTTL time.Duration
	Now        func() time.Time
}

// RetentionResult reports rows removed by an expiry pass or explicit clear.
type RetentionResult struct {
	Segments int
	Episodes int
}

// NewOptionsFromConfig maps the existing memory retention configuration to
// activity retention. Invalid or empty durations use safe defaults.
func NewOptionsFromConfig(segmentTTL, episodeTTL string) Options {
	return Options{
		SegmentTTL: parseDurationOr(segmentTTL, defaultSegmentTTL),
		EpisodeTTL: parseDurationOr(episodeTTL, defaultEpisodeTTL),
	}
}

func parseDurationOr(raw string, fallback time.Duration) time.Duration {
	if raw == "" {
		return fallback
	}
	// Accept Go's duration syntax. The config documentation uses "720h";
	// accepting days here would require a second parser and is unnecessary.
	if d, err := time.ParseDuration(raw); err == nil && d > 0 {
		return d
	}
	return fallback
}

func normalizeOptions(opts Options) Options {
	if opts.SegmentTTL <= 0 {
		opts.SegmentTTL = defaultSegmentTTL
	}
	if opts.EpisodeTTL <= 0 {
		opts.EpisodeTTL = defaultEpisodeTTL
	}
	if opts.Now == nil {
		opts.Now = func() time.Time { return time.Now().UTC() }
	}
	return opts
}

func (s Segment) validate() error {
	if s.ID == "" {
		return fmt.Errorf("activity segment id is required")
	}
	if strings.TrimSpace(s.Source) == "" {
		return fmt.Errorf("activity segment source is required")
	}
	if s.Granularity != Granularity10Min && s.Granularity != Granularity6Hour {
		return fmt.Errorf("invalid activity segment granularity %q", s.Granularity)
	}
	if s.StartedAt.IsZero() || s.EndedAt.IsZero() || !s.EndedAt.After(s.StartedAt) {
		return fmt.Errorf("activity segment requires an increasing time window")
	}
	if s.RedactedSummary == "" {
		return fmt.Errorf("activity segment summary is required")
	}
	if s.ExpiresAt.IsZero() {
		return fmt.Errorf("activity segment expiry is required")
	}
	return nil
}

func (e Episode) validate() error {
	if e.ID == "" {
		return fmt.Errorf("activity episode id is required")
	}
	if e.Title == "" {
		return fmt.Errorf("activity episode title is required")
	}
	if e.StartedAt.IsZero() || e.EndedAt.IsZero() || !e.EndedAt.After(e.StartedAt) {
		return fmt.Errorf("activity episode requires an increasing time window")
	}
	if e.Confidence < 0 || e.Confidence > 1 {
		return fmt.Errorf("activity episode confidence must be between 0 and 1")
	}
	if e.ExpiresAt.IsZero() {
		return fmt.Errorf("activity episode expiry is required")
	}
	return nil
}

func segmentID(source, granularity string, start, end time.Time) string {
	return stableID("segment", source, granularity, start.UTC().Format(time.RFC3339Nano), end.UTC().Format(time.RFC3339Nano))
}

func episodeID(source string, start, end time.Time) string {
	return stableID("episode", source, start.UTC().Format(time.RFC3339Nano), end.UTC().Format(time.RFC3339Nano))
}

func stableID(parts ...string) string {
	h := sha256.New()
	for _, part := range parts {
		_, _ = h.Write([]byte(part))
		_, _ = h.Write([]byte{0})
	}
	return "activity-" + hex.EncodeToString(h.Sum(nil))[:32]
}

func marshalStrings(values []string) (string, error) {
	if values == nil {
		values = []string{}
	}
	return string(mustJSON(values)), nil
}

func mustJSON(value any) []byte {
	encoded, err := json.Marshal(value)
	if err != nil {
		panic(fmt.Sprintf("activity JSON encoding failed: %v", err))
	}
	return encoded
}

func unmarshalStrings(raw string) ([]string, error) {
	var values []string
	if err := json.Unmarshal([]byte(raw), &values); err != nil {
		return nil, err
	}
	return values, nil
}

func uniqueSorted(values []string) []string {
	seen := make(map[string]struct{}, len(values))
	out := make([]string, 0, len(values))
	for _, value := range values {
		value = strings.TrimSpace(value)
		if value == "" {
			continue
		}
		if _, ok := seen[value]; ok {
			continue
		}
		seen[value] = struct{}{}
		out = append(out, value)
	}
	sort.Strings(out)
	return out
}
