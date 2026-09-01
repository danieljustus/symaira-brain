package harness

import (
	"bytes"
	"fmt"
)

// jsonDocument is the Document implementation for the five JSON-based
// harnesses (claude, claude-desktop, cursor, opencode, antigravity).
// It uses orderedMap instead of map[string]any so that key order from
// the original config file is preserved — without this, a dry-run diff
// showed thousands of lines of key reordering noise for a single-entry
// change.
type jsonDocument struct {
	root       *orderedMap
	serversKey string
}

func newJSONDocument(root *orderedMap, serversKey string) *jsonDocument {
	return &jsonDocument{root: root, serversKey: serversKey}
}

func (d *jsonDocument) servers() *orderedMap {
	v, ok := d.root.get(d.serversKey)
	if !ok {
		return nil
	}
	m, _ := v.(*orderedMap)
	return m
}

func (d *jsonDocument) Server(name string) (Entry, bool) {
	servers := d.servers()
	if servers == nil {
		return Entry{}, false
	}
	raw, ok := servers.get(name)
	if !ok {
		return Entry{}, false
	}
	m, ok := raw.(*orderedMap)
	if !ok {
		return Entry{}, false
	}
	// Convert *orderedMap back to map[string]any for entryFromMap.
	return entryFromOrderedMap(m), true
}

func (d *jsonDocument) ServerInfo(name string) (ServerInfo, bool) {
	servers := d.servers()
	if servers == nil {
		return ServerInfo{}, false
	}
	raw, ok := servers.get(name)
	if !ok {
		return ServerInfo{}, false
	}
	m, ok := raw.(*orderedMap)
	if !ok {
		return ServerInfo{}, false
	}
	return serverInfoFromOrderedMap(m, name), true
}

func (d *jsonDocument) ServerNames() []string {
	servers := d.servers()
	if servers == nil {
		return []string{}
	}
	return sortedNames(append([]string(nil), servers.keys...))
}

func (d *jsonDocument) SetServer(name string, entry Entry) {
	servers := d.servers()
	if servers == nil {
		servers = newOrderedMap()
		d.root.set(d.serversKey, servers)
	}
	servers.set(name, entryToOrderedMap(entry))
}

func (d *jsonDocument) RemoveServer(name string) bool {
	servers := d.servers()
	if servers == nil {
		return false
	}
	if _, ok := servers.get(name); !ok {
		return false
	}
	servers.del(name)
	if servers.len() == 0 {
		d.root.del(d.serversKey)
	}
	return true
}

// Marshal re-encodes the document with two-space indent, unescaped HTML
// characters, and key order preserved from the original config file.
func (d *jsonDocument) Marshal() ([]byte, error) {
	var buf bytes.Buffer
	if err := d.root.marshalJSON(&buf, "", "  "); err != nil {
		return nil, fmt.Errorf("encode json: %w", err)
	}
	// Append trailing newline to match the previous json.Encoder behavior,
	// which appends '\n' after every Encode call.
	buf.WriteByte('\n')
	return buf.Bytes(), nil
}

// entryFromOrderedMap reads an Entry out of an *orderedMap representation.
// Fields of an unexpected type are left zero rather than erroring — a
// foreign entry that doesn't match the shape symbrain writes simply reports
// as "not symbrain" via Entry.IsSymbrain.
func entryFromOrderedMap(m *orderedMap) Entry {
	var e Entry
	if c, ok := m.get("command"); ok {
		if s, ok := c.(string); ok {
			e.Command = s
		}
	}
	if rawArgs, ok := m.get("args"); ok {
		if arr, ok := rawArgs.([]any); ok {
			e.Args = make([]string, 0, len(arr))
			for _, a := range arr {
				if s, ok := a.(string); ok {
					e.Args = append(e.Args, s)
				}
			}
		}
	}
	return e
}
