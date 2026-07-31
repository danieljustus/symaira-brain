package harness

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
)

// orderedMap is a JSON object representation that preserves key insertion
// order. It is used by jsonDocument so that Parse → Marshal round-trips
// don't reorder keys — the root cause of unreviewable dry-run diffs where
// adding a single server entry produces thousands of lines of key reordering
// noise.
type orderedMap struct {
	keys   []string
	values map[string]any
}

func newOrderedMap() *orderedMap {
	return &orderedMap{values: make(map[string]any)}
}

func (m *orderedMap) get(key string) (any, bool) {
	v, ok := m.values[key]
	return v, ok
}

// set inserts or updates key. If key is new it is appended to the
// end of the key order; an existing key stays where it is.
func (m *orderedMap) set(key string, value any) {
	if _, exists := m.values[key]; !exists {
		m.keys = append(m.keys, key)
	}
	m.values[key] = value
}

func (m *orderedMap) del(key string) bool {
	if _, exists := m.values[key]; !exists {
		return false
	}
	delete(m.values, key)
	for i, k := range m.keys {
		if k == key {
			m.keys = append(m.keys[:i], m.keys[i+1:]...)
			break
		}
	}
	return true
}

func (m *orderedMap) len() int {
	return len(m.values)
}

// decodeJSONObject parses data as a single JSON object into an *orderedMap.
// The decoder uses json.Number so numeric values round-trip with their
// original textual form. Nested objects are also decoded as *orderedMap to
// preserve key order at every level. Trailing garbage after the top-level
// object is rejected.
func decodeJSONObject(data []byte) (*orderedMap, error) {
	dec := json.NewDecoder(bytes.NewReader(data))
	dec.UseNumber()

	root, err := decodeOrderedMap(dec)
	if err != nil {
		return nil, fmt.Errorf("parse json: %w", err)
	}

	if dec.More() {
		return nil, fmt.Errorf("parse json: unexpected trailing content after the top-level value")
	}

	return root, nil
}

// decodeOrderedMap reads a JSON object from dec into an *orderedMap.
// The caller must have already consumed the opening '{' delimiter, or this
// consumes it and the matching '}'.
func decodeOrderedMap(dec *json.Decoder) (*orderedMap, error) {
	// Read opening brace.
	t, err := dec.Token()
	if err != nil {
		return nil, err
	}
	if delim, ok := t.(json.Delim); !ok || delim != '{' {
		return nil, fmt.Errorf("expected {, got %v", t)
	}

	om := newOrderedMap()
	for dec.More() {
		t, err := dec.Token()
		if err != nil {
			return nil, err
		}
		key, ok := t.(string)
		if !ok {
			return nil, fmt.Errorf("expected string key, got %v", t)
		}

		val, err := decodeJSONValue(dec)
		if err != nil {
			return nil, err
		}
		om.set(key, val)
	}

	// Read closing brace.
	t, err = dec.Token()
	if err != nil {
		return nil, err
	}
	if delim, ok := t.(json.Delim); !ok || delim != '}' {
		return nil, fmt.Errorf("expected }, got %v", t)
	}

	return om, nil
}

// decodeJSONValue reads the next JSON value from dec and returns it.
// Objects become *orderedMap, arrays become []any, strings become string,
// numbers become json.Number, booleans become bool, null becomes nil.
func decodeJSONValue(dec *json.Decoder) (any, error) {
	t, err := dec.Token()
	if err != nil {
		return nil, err
	}

	switch v := t.(type) {
	case json.Delim:
		switch v {
		case '{':
			om := newOrderedMap()
			for dec.More() {
				t, err := dec.Token()
				if err != nil {
					return nil, err
				}
				key, ok := t.(string)
				if !ok {
					return nil, fmt.Errorf("expected string key, got %v", t)
				}
				val, err := decodeJSONValue(dec)
				if err != nil {
					return nil, err
				}
				om.set(key, val)
			}
			// Consume closing }
			if _, err := dec.Token(); err != nil {
				return nil, err
			}
			return om, nil
		case '[':
			var arr []any
			for dec.More() {
				val, err := decodeJSONValue(dec)
				if err != nil {
					return nil, err
				}
				arr = append(arr, val)
			}
			// Consume closing ]
			if _, err := dec.Token(); err != nil {
				return nil, err
			}
			return arr, nil
		default:
			return nil, fmt.Errorf("unexpected delimiter: %v", v)
		}
	case json.Number:
		return v, nil
	default:
		return v, nil
	}
}

// marshalJSON writes the ordered map as indented JSON to w. This is used
// instead of json.Encoder so we control key order. Two-space indent and
// no HTML escaping match the previous encoder behavior.
func (m *orderedMap) marshalJSON(w io.Writer, prefix, indent string) error {
	write := func(s string) { io.WriteString(w, s) }

	if m == nil || len(m.keys) == 0 {
		write("{}")
		return nil
	}

	write("{\n")
	for i, key := range m.keys {
		write(prefix)
		write(indent)

		// Key
		keyJSON, err := json.Marshal(key)
		if err != nil {
			return err
		}
		write(string(keyJSON))
		write(": ")

		// Value
		if err := marshalJSONValue(w, m.values[key], prefix+indent, indent); err != nil {
			return fmt.Errorf("marshal key %q: %w", key, err)
		}

		if i < len(m.keys)-1 {
			write(",")
		}
		write("\n")
	}
	write(prefix)
	write("}")
	return nil
}

// marshalJSONValue writes a JSON value to w, handling *orderedMap for
// nested objects. Other types fall through to json.Marshal.
func marshalJSONValue(w io.Writer, v any, prefix, indent string) error {
	if v == nil {
		io.WriteString(w, "null")
		return nil
	}

	switch val := v.(type) {
	case *orderedMap:
		return val.marshalJSON(w, prefix, indent)
	case []any:
		return marshalJSONArray(w, val, prefix, indent)
	case json.Number:
		io.WriteString(w, val.String())
		return nil
	case string:
		return writeJSONString(w, val)
	case bool:
		if val {
			io.WriteString(w, "true")
		} else {
			io.WriteString(w, "false")
		}
		return nil
	default:
		// Fallback for any types we didn't explicitly handle (should
		// not occur in normal operation since decodeJSONValue only
		// produces the types above).
		b, err := json.Marshal(v)
		if err != nil {
			return err
		}
		_, err = w.Write(b)
		return err
	}
}

// marshalJSONArray writes a JSON array to w.
func marshalJSONArray(w io.Writer, arr []any, prefix, indent string) error {
	if len(arr) == 0 {
		io.WriteString(w, "[]")
		return nil
	}

	io.WriteString(w, "[\n")
	for i, val := range arr {
		io.WriteString(w, prefix)
		io.WriteString(w, indent)
		if err := marshalJSONValue(w, val, prefix+indent, indent); err != nil {
			return err
		}
		if i < len(arr)-1 {
			io.WriteString(w, ",")
		}
		io.WriteString(w, "\n")
	}
	io.WriteString(w, prefix)
	io.WriteString(w, "]")
	return nil
}

// writeJSONString writes a Go string as a JSON string literal to w. HTML
// characters (<, >, &) are NOT escaped — matching SetEscapeHTML(false) on
// the standard encoder.
func writeJSONString(w io.Writer, s string) error {
	io.WriteString(w, "\"")
	for _, r := range s {
		switch r {
		case '"':
			io.WriteString(w, `\"`)
		case '\\':
			io.WriteString(w, `\\`)
		case '\b':
			io.WriteString(w, `\b`)
		case '\f':
			io.WriteString(w, `\f`)
		case '\n':
			io.WriteString(w, `\n`)
		case '\r':
			io.WriteString(w, `\r`)
		case '\t':
			io.WriteString(w, `\t`)
		default:
			if r < 0x20 {
				fmt.Fprintf(w, `\u%04x`, r)
			} else {
				// Write the rune directly as UTF-8 — HTML
				// characters are NOT escaped here.
				buf := make([]byte, 4)
				n := 0
				switch {
				case r <= 0x7F:
					buf[0] = byte(r)
					n = 1
				case r <= 0x7FF:
					buf[0] = byte(0xC0 | (r >> 6))
					buf[1] = byte(0x80 | (r & 0x3F))
					n = 2
				case r <= 0xFFFF:
					// Handle surrogate pairs
					buf[0] = byte(0xE0 | (r >> 12))
					buf[1] = byte(0x80 | ((r >> 6) & 0x3F))
					buf[2] = byte(0x80 | (r & 0x3F))
					n = 3
				default:
					buf[0] = byte(0xF0 | (r >> 18))
					buf[1] = byte(0x80 | ((r >> 12) & 0x3F))
					buf[2] = byte(0x80 | ((r >> 6) & 0x3F))
					buf[3] = byte(0x80 | (r & 0x3F))
					n = 4
				}
				w.Write(buf[:n])
			}
		}
	}
	io.WriteString(w, "\"")
	return nil
}

// toOrderedMap converts a map[string]any to *orderedMap, preserving the
// Go map's iteration order (which is undefined but consistent within a
// single process). Used only when constructing new entries from scratch
// (SetServer), where the map has two keys in a known order. For TOML
// interop, it shallow-converts the first level of nested maps.
func toOrderedMap(m map[string]any) *orderedMap {
	om := newOrderedMap()
	for k, v := range m {
		// For the entry map {"command": ..., "args": ...}, we want
		// "command" before "args" consistently. Go map iteration is
		// not specified, but for small maps with known keys we can
		// enforce order explicitly.
		om.set(k, v)
	}
	return om
}

// entryToOrderedMap is like entryToMap but returns an *orderedMap with a
// stable key order ("command" first, then "args") for use in jsonDocument.
func entryToOrderedMap(e Entry) *orderedMap {
	args := make([]any, len(e.Args))
	for i, a := range e.Args {
		args[i] = a
	}
	om := newOrderedMap()
	om.set("command", e.Command)
	om.set("args", args)
	return om
}
