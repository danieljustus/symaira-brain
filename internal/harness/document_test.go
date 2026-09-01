package harness

import (
	"errors"
	"os"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-corekit/exitcodes"
)

func TestParse_CorruptJSON_ReturnsTypedError(t *testing.T) {
	h, err := Lookup("claude")
	if err != nil {
		t.Fatalf("Lookup: %v", err)
	}

	cases := []struct {
		name string
		data string
	}{
		{"not json at all", "this is not json"},
		{"truncated object", `{"mcpServers": {`},
		{"top-level array", `["mcpServers"]`},
		{"trailing garbage", `{}garbage`},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			_, err := Parse(h, []byte(tc.data))
			if err == nil {
				t.Fatal("Parse() = nil error, want a parse error")
			}
			var cliErr *exitcodes.CLIError
			if !errors.As(err, &cliErr) {
				t.Fatalf("Parse() error is not an *exitcodes.CLIError: %v (%T)", err, err)
			}
			if cliErr.Code != exitcodes.ExitNoInput {
				t.Errorf("Code = %d, want %d (ExitNoInput)", cliErr.Code, exitcodes.ExitNoInput)
			}
			if cliErr.Kind != exitcodes.KindConfig {
				t.Errorf("Kind = %q, want %q", cliErr.Kind, exitcodes.KindConfig)
			}
		})
	}
}

func TestParse_CorruptTOML_ReturnsTypedError(t *testing.T) {
	h, err := Lookup("codex")
	if err != nil {
		t.Fatalf("Lookup: %v", err)
	}

	cases := []struct {
		name string
		data string
	}{
		{"unterminated table", "[mcp_servers"},
		{"bad key-value", "not = = valid"},
		{"duplicate keys", "a = 1\na = 2\n"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			_, err := Parse(h, []byte(tc.data))
			if err == nil {
				t.Fatal("Parse() = nil error, want a parse error")
			}
			var cliErr *exitcodes.CLIError
			if !errors.As(err, &cliErr) {
				t.Fatalf("Parse() error is not an *exitcodes.CLIError: %v (%T)", err, err)
			}
			if cliErr.Code != exitcodes.ExitNoInput {
				t.Errorf("Code = %d, want %d (ExitNoInput)", cliErr.Code, exitcodes.ExitNoInput)
			}
		})
	}
}

func TestParse_EmptyObjectIsValid(t *testing.T) {
	// Discarding the Lookup error here used to hide the gemini -> antigravity
	// rename: the lookup failed, h stayed a zero-value Harness, and Parse still
	// passed via its JSON default. Assert the lookup so the next rename fails
	// the test instead of quietly changing what it covers.
	h, err := Lookup("antigravity")
	if err != nil {
		t.Fatalf("Lookup: %v", err)
	}
	doc, err := Parse(h, []byte("{}"))
	if err != nil {
		t.Fatalf("Parse(\"{}\"): %v", err)
	}
	if _, ok := doc.Server(ServerName); ok {
		t.Error("Server() found an entry in an empty document")
	}
}

func TestDocument_SetServer_ThenServer_RoundTrips(t *testing.T) {
	h, _ := Lookup("claude")
	doc := Empty(h)

	entry := NewEntry("personal")
	doc.SetServer(ServerName, entry)

	got, ok := doc.Server(ServerName)
	if !ok {
		t.Fatal("Server() ok = false after SetServer")
	}
	if got.Command != entry.Command {
		t.Errorf("Command = %q, want %q", got.Command, entry.Command)
	}
	if len(got.Args) != len(entry.Args) {
		t.Fatalf("Args = %v, want %v", got.Args, entry.Args)
	}
	for i := range entry.Args {
		if got.Args[i] != entry.Args[i] {
			t.Errorf("Args[%d] = %q, want %q", i, got.Args[i], entry.Args[i])
		}
	}
}

func TestDocument_RemoveServer_NotPresent(t *testing.T) {
	h, _ := Lookup("claude")
	doc := Empty(h)

	if removed := doc.RemoveServer(ServerName); removed {
		t.Error("RemoveServer() on empty document = true, want false")
	}
}

func TestDocument_RemoveServer_DropsEmptyContainerKey(t *testing.T) {
	h, _ := Lookup("cursor")
	doc := Empty(h)
	doc.SetServer(ServerName, NewEntry("personal"))

	if removed := doc.RemoveServer(ServerName); !removed {
		t.Fatal("RemoveServer() = false, want true")
	}

	data, err := doc.Marshal()
	if err != nil {
		t.Fatalf("Marshal: %v", err)
	}
	// The servers container key must be gone entirely, not left behind as
	// an empty object/table, so install+uninstall round-trips exactly back
	// to a config that never had the key.
	if string(data) != "{}\n" {
		t.Errorf("Marshal() = %q, want %q (mcpServers key dropped)", data, "{}\n")
	}
}

func TestDocument_RemoveServer_LeavesOtherEntriesAlone(t *testing.T) {
	h, _ := Lookup("cursor")
	doc := Empty(h)
	doc.SetServer("other-tool", Entry{Command: "other-tool", Args: []string{"run"}})
	doc.SetServer(ServerName, NewEntry("personal"))

	if removed := doc.RemoveServer(ServerName); !removed {
		t.Fatal("RemoveServer() = false, want true")
	}

	if _, ok := doc.Server(ServerName); ok {
		t.Error("symbrain entry still present after RemoveServer")
	}
	other, ok := doc.Server("other-tool")
	if !ok {
		t.Fatal("unrelated server entry was removed too")
	}
	if other.Command != "other-tool" {
		t.Errorf("other-tool Command = %q, want %q", other.Command, "other-tool")
	}
}

func TestLoad_MissingFile_ReturnsNotExist(t *testing.T) {
	h, _ := Lookup("claude")
	_, err := Load(h, "/nonexistent/path/that/should/not/exist.json")
	if err == nil {
		t.Fatal("Load() on missing file = nil error, want an error")
	}
	if !os.IsNotExist(err) {
		t.Errorf("Load() error = %v, want os.IsNotExist to be true", err)
	}
}

// TestParse_EmptyFileIsAnEmptyDocument covers the branch that lets symbrain
// install into a harness whose config file exists but was never written to.
// Antigravity ships an empty ~/.gemini/config/mcp_config.json and `agy mcp
// list` reads it as "no MCP servers configured"; treating that as a parse
// error would refuse the install on a fresh setup. Malformed JSON must still
// be rejected, which TestParse_CorruptJSON_ReturnsTypedError pins.
func TestParse_EmptyFileIsAnEmptyDocument(t *testing.T) {
	h, err := Lookup("antigravity")
	if err != nil {
		t.Fatalf("Lookup: %v", err)
	}

	for name, data := range map[string][]byte{
		"empty":      {},
		"whitespace": []byte("   \n\t\n"),
	} {
		t.Run(name, func(t *testing.T) {
			doc, err := Parse(h, data)
			if err != nil {
				t.Fatalf("Parse(%q) = %v, want an empty document", data, err)
			}
			if names := doc.ServerNames(); len(names) != 0 {
				t.Fatalf("ServerNames() = %v, want none", names)
			}

			// The document must be writable, not merely parseable — that is
			// the whole point of accepting the empty file.
			doc.SetServer(ServerName, Entry{Command: "symbrain", Args: []string{"mcp", "--profile", "personal"}})
			out, err := doc.Marshal()
			if err != nil {
				t.Fatalf("Marshal: %v", err)
			}
			if !strings.Contains(string(out), `"symbrain"`) {
				t.Fatalf("marshalled config does not carry the entry: %s", out)
			}
			if !strings.Contains(string(out), `"mcpServers"`) {
				t.Fatalf("marshalled config missing the servers key: %s", out)
			}
		})
	}
}
