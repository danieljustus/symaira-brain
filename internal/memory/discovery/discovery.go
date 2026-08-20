package discovery

import (
	"sort"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
)

const SchemaVersion = 1

// Provider describes one logical source that can be scanned without importing
// or mutating any state.
type Provider struct {
	// ID is a stable logical identity, not a filesystem path. This keeps the
	// source ID stable when a configured source moves.
	ID          string
	Tool        string
	Kind        string
	DisplayName string
	Location    string
	PrivacyHint string
	Discover    func() ([]importer.SessionRef, error)
}

// Source is the metadata-only representation exposed to Hub clients.
type Source struct {
	SourceID     string   `json:"source_id"`
	Tool         string   `json:"tool"`
	Kind         string   `json:"kind"`
	DisplayName  string   `json:"display_name"`
	Location     string   `json:"location"`
	Capabilities []string `json:"capabilities"`
	ItemCount    int      `json:"item_count,omitempty"`
	LastSeen     string   `json:"last_seen,omitempty"`
	PrivacyHint  string   `json:"privacy_hint,omitempty"`
}

// Response is the versioned source-discovery envelope.
type Response struct {
	SchemaVersion int      `json:"schema_version"`
	Sources       []Source `json:"sources"`
}

type sourceGroup struct {
	provider Provider
	refs     map[string]importer.SessionRef
}

// Scan discovers sources from all providers. Individual provider failures are
// intentionally ignored: discovery is best-effort and a missing/unavailable
// source must not hide sources from other providers.
func Scan(providers []Provider, now time.Time) Response {
	groups := make(map[string]*sourceGroup, len(providers))

	for _, provider := range providers {
		if provider.ID == "" || provider.Discover == nil {
			continue
		}

		refs, err := provider.Discover()
		if err != nil {
			continue
		}

		id := "symmemory:" + provider.ID
		group := groups[id]
		if group == nil {
			group = &sourceGroup{provider: provider, refs: make(map[string]importer.SessionRef)}
			groups[id] = group
		}

		for _, ref := range refs {
			key := ref.Tool + "\x00" + ref.SessionID
			if ref.SessionID == "" {
				key = ref.Tool + "\x00" + ref.Path
			}
			group.refs[key] = ref
		}
	}

	sources := make([]Source, 0, len(groups))
	for id, group := range groups {
		if len(group.refs) == 0 {
			continue
		}

		lastSeen := time.Time{}
		for _, ref := range group.refs {
			if ref.ModifiedAt.After(lastSeen) {
				lastSeen = ref.ModifiedAt
			}
		}
		if lastSeen.IsZero() {
			lastSeen = now
		}

		sources = append(sources, Source{
			SourceID:     id,
			Tool:         group.provider.Tool,
			Kind:         group.provider.Kind,
			DisplayName:  group.provider.DisplayName,
			Location:     group.provider.Location,
			Capabilities: []string{"import"},
			ItemCount:    len(group.refs),
			LastSeen:     lastSeen.UTC().Format(time.RFC3339),
			PrivacyHint:  group.provider.PrivacyHint,
		})
	}

	sort.Slice(sources, func(i, j int) bool {
		if sources[i].Tool != sources[j].Tool {
			return sources[i].Tool < sources[j].Tool
		}
		return sources[i].SourceID < sources[j].SourceID
	})

	return Response{SchemaVersion: SchemaVersion, Sources: sources}
}
