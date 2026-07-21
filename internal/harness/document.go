package harness

import (
	"fmt"
	"os"

	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// Document is a parsed harness config file that can be inspected and
// structurally edited (insert/remove the symbrain MCP entry) without
// disturbing any other content in the file. Implementations never use
// regular expressions or text splicing — every edit goes through the
// format's real parser/encoder (encoding/json or BurntSushi/toml) so
// unrelated keys, servers, and nesting survive untouched.
type Document interface {
	// Server returns the entry registered under serverName in this
	// document's server map, and whether one exists.
	Server(serverName string) (Entry, bool)
	// SetServer inserts or overwrites the entry for serverName.
	SetServer(serverName string, entry Entry)
	// RemoveServer deletes serverName if present, reporting whether
	// anything was actually removed. If removing serverName leaves the
	// server map empty, the (previously absent) container key is dropped
	// too, so a plain install+uninstall round-trip restores the original
	// file byte-for-byte.
	RemoveServer(serverName string) bool
	// Marshal serializes the document back to its on-disk representation,
	// deterministically (stable key ordering) so the same logical content
	// always produces the same bytes.
	Marshal() ([]byte, error)
}

// Empty returns a fresh, empty document in h's format, for the common case
// where no config file exists yet at the target path.
func Empty(h Harness) Document {
	switch h.Format {
	case FormatTOML:
		return newTOMLDocument(map[string]any{}, h.ServersKey)
	default:
		return newJSONDocument(map[string]any{}, h.ServersKey)
	}
}

// Parse decodes data as h's config format and returns an editable Document.
// A file that fails to parse is reported as a typed *exitcodes.CLIError
// (ExitNoInput, KindConfig) — install and uninstall use this to refuse a
// harness config they cannot safely edit rather than attempt to repair it.
func Parse(h Harness, data []byte) (Document, error) {
	switch h.Format {
	case FormatTOML:
		root, err := decodeTOMLObject(data)
		if err != nil {
			return nil, wrapParseError(h, err)
		}
		return newTOMLDocument(root, h.ServersKey), nil
	default:
		root, err := decodeJSONObject(data)
		if err != nil {
			return nil, wrapParseError(h, err)
		}
		return newJSONDocument(root, h.ServersKey), nil
	}
}

// Load reads and parses the config file for h at path. A missing file is
// returned unwrapped so callers can distinguish it with os.IsNotExist and
// fall back to Empty(h).
func Load(h Harness, path string) (Document, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	return Parse(h, data)
}

func wrapParseError(h Harness, err error) error {
	return exitcodes.Wrap(err, exitcodes.ExitNoInput, exitcodes.KindConfig,
		fmt.Sprintf("harness: %s config is not valid %s; refusing to edit a config symbrain cannot parse", h.Name, h.Format))
}

// entryToMap converts an Entry to the generic map representation stored in
// a document's server table, shared by both the JSON and TOML backends.
func entryToMap(e Entry) map[string]any {
	args := make([]any, len(e.Args))
	for i, a := range e.Args {
		args[i] = a
	}
	return map[string]any{
		"command": e.Command,
		"args":    args,
	}
}

// entryFromMap reads an Entry back out of a document's generic server-table
// representation. Fields of an unexpected type are left zero rather than
// erroring — a foreign entry that doesn't match the shape symbrain writes
// simply reports as "not symbrain" via Entry.IsSymbrain.
func entryFromMap(m map[string]any) Entry {
	var e Entry
	if c, ok := m["command"].(string); ok {
		e.Command = c
	}
	if rawArgs, ok := m["args"].([]any); ok {
		e.Args = make([]string, 0, len(rawArgs))
		for _, a := range rawArgs {
			if s, ok := a.(string); ok {
				e.Args = append(e.Args, s)
			}
		}
	}
	return e
}
