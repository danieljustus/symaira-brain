// Package recipes turns repeated gateway sessions into reviewable,
// versioned recipe artifacts (issue #186).
//
// An episode is the ordered tool-call sequence of one harness connection
// as seen by the gateway routing layer — names only, never arguments or
// values. A candidate is promoted to an exposable recipe only after the
// same sequence recurs across a configurable number of sessions; the
// promotion gate is what keeps the store from filling with one-off
// noise. Brain exposes recipes as read-only context; it never executes
// them, and anything that stabilizes into a durable authored artifact
// belongs to symskills, not here.
package recipes

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"sort"
	"strings"
)

// Step is one recorded tool invocation within an episode. It carries
// names only — no arguments, no values, no results.
type Step struct {
	Server string `json:"server"`
	Tool   string `json:"tool"`
}

// Episode is the ordered tool-call sequence of one completed gateway
// session (one ServeIO connection) for one profile.
type Episode struct {
	Profile   string `json:"profile"`
	Steps     []Step `json:"steps"`
	StartedAt string `json:"started_at"`
	EndedAt   string `json:"ended_at"`
}

// Trigger states when a recipe applies: the profile and the first step
// of the recorded sequence.
type Trigger struct {
	Profile   string `json:"profile"`
	FirstStep Step   `json:"first_step"`
}

// Provenance records where a recipe came from, so a promoted recipe is
// reviewable rather than an anonymous blob.
type Provenance struct {
	FirstSeenAt     string `json:"first_seen_at"`
	LastSeenAt      string `json:"last_seen_at"`
	RecurrenceCount int    `json:"recurrence_count"`
}

// Recipe is a promoted, named, versioned artifact: the steps of a
// recurring episode plus its trigger conditions and provenance.
type Recipe struct {
	Name       string     `json:"name"`
	Version    int        `json:"version"`
	Profile    string     `json:"profile"`
	Steps      []Step     `json:"steps"`
	Trigger    Trigger    `json:"trigger"`
	Provenance Provenance `json:"provenance"`
}

// sequenceKey identifies an episode for recurrence counting: profile
// plus the exact ordered step sequence. Order matters — the same tools
// in a different order are a different approach, not a recurrence.
func sequenceKey(profile string, steps []Step) string {
	var sb strings.Builder
	sb.WriteString(profile)
	sb.WriteByte('\x00')
	for _, s := range steps {
		sb.WriteString(s.Server)
		sb.WriteByte('/')
		sb.WriteString(s.Tool)
		sb.WriteByte('\x00')
	}
	return sb.String()
}

// Name derives a deterministic, human-readable recipe name from the
// profile and first steps, disambiguated by a short hash of the full
// sequence so distinct sequences never collide.
func Name(profile string, steps []Step) string {
	slugParts := []string{profile}
	for i, s := range steps {
		if i >= 3 {
			break
		}
		slugParts = append(slugParts, s.Server+"_"+s.Tool)
	}
	slug := strings.Join(slugParts, "_")

	sum := sha256.Sum256([]byte(sequenceKey(profile, steps)))
	return fmt.Sprintf("%s_%s", slug, hex.EncodeToString(sum[:])[:8])
}

// Promote returns the recipes whose exact sequence recurred in at least
// threshold distinct episodes, sorted by name for deterministic output.
// Episodes with fewer than two steps are never promoted (a single tool
// call is not an approach). The earliest and latest occurrences feed the
// provenance record.
func Promote(episodes []Episode, threshold int) []Recipe {
	type acc struct {
		profile string
		steps   []Step
		first   string
		last    string
		count   int
	}

	groups := make(map[string]*acc)
	var order []string

	for _, ep := range episodes {
		if len(ep.Steps) < 2 {
			continue
		}
		key := sequenceKey(ep.Profile, ep.Steps)
		a, ok := groups[key]
		if !ok {
			a = &acc{profile: ep.Profile, steps: ep.Steps, first: ep.StartedAt}
			groups[key] = a
			order = append(order, key)
		}
		a.count++
		a.last = ep.EndedAt
	}

	var recipes []Recipe
	for _, key := range order {
		a := groups[key]
		if a.count < threshold {
			continue
		}
		recipes = append(recipes, Recipe{
			Name:    Name(a.profile, a.steps),
			Version: 1,
			Profile: a.profile,
			Steps:   append([]Step(nil), a.steps...),
			Trigger: Trigger{
				Profile:   a.profile,
				FirstStep: a.steps[0],
			},
			Provenance: Provenance{
				FirstSeenAt:     a.first,
				LastSeenAt:      a.last,
				RecurrenceCount: a.count,
			},
		})
	}

	sort.Slice(recipes, func(i, j int) bool { return recipes[i].Name < recipes[j].Name })
	return recipes
}
