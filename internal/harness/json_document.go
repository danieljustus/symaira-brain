package harness

import (
	"bytes"
	"encoding/json"
	"fmt"
)

// jsonDocument is the Document implementation for the five JSON-based
// harnesses (claude, claude-desktop, cursor, opencode, gemini).
type jsonDocument struct {
	genericDocument
}

func newJSONDocument(root map[string]any, serversKey string) *jsonDocument {
	return &jsonDocument{genericDocument{root: root, serversKey: serversKey}}
}

// decodeJSONObject parses data as a single JSON object, rejecting anything
// else (arrays, scalars, trailing garbage) as an unsupported/corrupt config.
// json.Number is used for numeric literals so that a value symbrain never
// touches (e.g. a port number) round-trips through Marshal with its
// original textual form intact.
func decodeJSONObject(data []byte) (map[string]any, error) {
	dec := json.NewDecoder(bytes.NewReader(data))
	dec.UseNumber()

	var root map[string]any
	if err := dec.Decode(&root); err != nil {
		return nil, fmt.Errorf("parse json: %w", err)
	}
	if dec.More() {
		return nil, fmt.Errorf("parse json: unexpected trailing content after the top-level value")
	}
	if root == nil {
		root = map[string]any{}
	}
	return root, nil
}

// Marshal re-encodes the document deterministically: two-space indent,
// unescaped HTML characters, and Go's stable (alphabetical) map key
// ordering — the same rendering used to build the golden fixtures, so a
// config that starts in that canonical form round-trips byte-for-byte
// through an install followed by an uninstall.
func (d *jsonDocument) Marshal() ([]byte, error) {
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	enc.SetEscapeHTML(false)
	enc.SetIndent("", "  ")
	if err := enc.Encode(d.root); err != nil {
		return nil, fmt.Errorf("encode json: %w", err)
	}
	return buf.Bytes(), nil
}
