package catalog

import (
	"encoding/json"
	"fmt"
	"sort"

	"github.com/danieljustus/symaira-brain/internal/policy"
)

// Tool mirrors broker.Tool but avoids an import cycle. The gateway
// translates between the two when building the catalog.
type Tool struct {
	Name        string          `json:"name"`
	Description string          `json:"description,omitempty"`
	InputSchema json.RawMessage `json:"inputSchema,omitempty"`
}

// Entry is one tool in the merged catalog, annotated with its source
// server and the policy verdict that determined its exposure.
type Entry struct {
	Tool
	Server  string         `json:"server"`
	Verdict policy.Verdict `json:"verdict"`
}

// Catalog is the merged, namespaced, policy-filtered tool list presented
// to the harness. It is built once at gateway startup and is immutable
// afterward.
type Catalog struct {
	entries []Entry
	byName  map[string]int
}

// CollisionError is returned when two different servers produce the same
// tool name after namespacing. This is a hard startup error per the
// issue requirements (stable names are required for downstream schema
// pinning by symguard).
type CollisionError struct {
	Name     string
	Server   string
	Existing string
}

func (e *CollisionError) Error() string {
	return fmt.Sprintf(
		"catalog: tool name %q collision: already registered by server %q, cannot add from %q",
		e.Name, e.Existing, e.Server)
}

// Build constructs a Catalog from the per-server tool lists and their
// policy reports. Each server's tools are namespaced according to its
// prefix, then merged into a single deterministic list.
//
// Build returns an error if any tool name collides after prefixing.
func Build(servers []ServerTools) (*Catalog, error) {
	var entries []Entry
	seen := make(map[string]string) // name → server that owns it

	for _, st := range servers {
		prefix := serverPrefix(st.Server)

		for _, tool := range st.Tools {
			name := namespace(tool.Name, prefix)

			if existing, ok := seen[name]; ok {
				return nil, &CollisionError{
					Name:     name,
					Server:   st.Server,
					Existing: existing,
				}
			}
			seen[name] = st.Server

			// Evaluate policy against the original (unprefixed) tool name.
			verdict := st.Report.Verdict(tool.Name)
			entries = append(entries, Entry{
				Tool:    Tool{Name: name, Description: tool.Description, InputSchema: tool.InputSchema},
				Server:  st.Server,
				Verdict: verdict,
			})
		}
	}

	sort.Slice(entries, func(i, j int) bool {
		return entries[i].Name < entries[j].Name
	})

	byName := make(map[string]int, len(entries))
	for i, e := range entries {
		byName[e.Name] = i
	}

	return &Catalog{entries: entries, byName: byName}, nil
}

// ServerTools pairs a server's live tool list with its policy report for
// catalog construction.
type ServerTools struct {
	Server string
	Tools  []Tool
	Report *policy.Report
}

// Exposed returns only the tools with an Exposed verdict, sorted by name.
func (c *Catalog) Exposed() []Entry {
	var result []Entry
	for _, e := range c.entries {
		if e.Verdict == policy.Exposed {
			result = append(result, e)
		}
	}
	return result
}

// All returns every entry (exposed, hidden, unknown) for diagnostic
// output (e.g. `symbrain profile show`).
func (c *Catalog) All() []Entry {
	return append([]Entry(nil), c.entries...)
}

// Lookup returns the Entry for the given tool name, or nil if not found.
func (c *Catalog) Lookup(name string) *Entry {
	idx, ok := c.byName[name]
	if !ok {
		return nil
	}
	e := c.entries[idx]
	return &e
}

// Names returns the sorted list of exposed tool names, suitable for
// tools/list responses.
func (c *Catalog) Names() []string {
	exposed := c.Exposed()
	names := make([]string, len(exposed))
	for i, e := range exposed {
		names[i] = e.Name
	}
	return names
}

// serverPrefix returns the namespace prefix for a server. Symvault
// tools get "vault_" by default; memory and skills tools are already
// namespaced upstream and pass through unchanged.
func serverPrefix(server string) string {
	switch server {
	case "vault":
		return "vault_"
	default:
		return ""
	}
}

// namespace applies the prefix to a tool name unless the name is already
// prefixed (pass-through for memory_*, entity_*, graph_*, skills tools).
func namespace(name, prefix string) string {
	if prefix == "" {
		return name
	}
	for _, p := range []string{"vault_", "memory_", "entity_", "graph_"} {
		if len(name) > len(p) && name[:len(p)] == p {
			return name
		}
	}
	return prefix + name
}
