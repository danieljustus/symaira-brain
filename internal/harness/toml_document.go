package harness

import (
	"bytes"
	"fmt"

	"github.com/BurntSushi/toml"
)

// tomlDocument is the Document implementation for codex's config.toml.
type tomlDocument struct {
	genericDocument
}

func newTOMLDocument(root map[string]any, serversKey string) *tomlDocument {
	return &tomlDocument{genericDocument{root: root, serversKey: serversKey}}
}

// decodeTOMLObject parses data as a TOML document into a generic tree.
func decodeTOMLObject(data []byte) (map[string]any, error) {
	var root map[string]any
	if err := toml.Unmarshal(data, &root); err != nil {
		return nil, fmt.Errorf("parse toml: %w", err)
	}
	if root == nil {
		root = map[string]any{}
	}
	return root, nil
}

// Marshal re-encodes the document via BurntSushi/toml's encoder, which
// sorts map keys alphabetically for deterministic output — matching the
// golden fixtures, so a canonically-formatted config round-trips
// byte-for-byte through an install followed by an uninstall.
func (d *tomlDocument) Marshal() ([]byte, error) {
	var buf bytes.Buffer
	if err := toml.NewEncoder(&buf).Encode(d.root); err != nil {
		return nil, fmt.Errorf("encode toml: %w", err)
	}
	return buf.Bytes(), nil
}
