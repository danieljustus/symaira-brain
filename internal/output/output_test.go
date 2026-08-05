package output

import (
	"bytes"
	"encoding/json"
	"io"
	"strings"
	"testing"
)

func TestResolveDefaultsToTableAndRecognizesJSON(t *testing.T) {
	for _, test := range []struct {
		name     string
		explicit string
		want     Format
	}{
		{name: "empty", explicit: "", want: FormatTable},
		{name: "table", explicit: "table", want: FormatTable},
		{name: "json", explicit: "json", want: FormatJSON},
		{name: "case insensitive", explicit: "JSON", want: FormatJSON},
		{name: "unknown is safe table", explicit: "future", want: FormatTable},
	} {
		t.Run(test.name, func(t *testing.T) {
			if got := Resolve(test.explicit); got != test.want {
				t.Fatalf("Resolve(%q) = %q, want %q", test.explicit, got, test.want)
			}
		})
	}
}

func TestParseRejectsUnknownFormats(t *testing.T) {
	if _, err := Parse("yaml"); err == nil {
		t.Fatal("Parse(yaml) returned nil error")
	}
}

func TestParseAcceptsEmptyAndTableFormats(t *testing.T) {
	for _, explicit := range []string{"", "table", " TABLE "} {
		got, err := Parse(explicit)
		if err != nil {
			t.Fatalf("Parse(%q): %v", explicit, err)
		}
		if got != FormatTable {
			t.Errorf("Parse(%q) = %q, want %q", explicit, got, FormatTable)
		}
	}
}

func TestExtractAcceptsGlobalFlagsAnywhere(t *testing.T) {
	format, args, err := Extract([]string{"profile", "show", "name", "--json"})
	if err != nil {
		t.Fatalf("Extract: %v", err)
	}
	if format != FormatJSON {
		t.Fatalf("format = %q, want %q", format, FormatJSON)
	}
	if got := strings.Join(args, " "); got != "profile show name" {
		t.Fatalf("args = %q, want %q", got, "profile show name")
	}

	format, args, err = Extract([]string{"--output", "json", "version"})
	if err != nil {
		t.Fatalf("Extract --output: %v", err)
	}
	if format != FormatJSON || strings.Join(args, " ") != "version" {
		t.Fatalf("Extract --output = (%q, %q), want (json, version)", format, strings.Join(args, " "))
	}
}

func TestExtractRejectsInvalidAndConflictingFlags(t *testing.T) {
	for _, test := range []struct {
		name string
		args []string
	}{
		{name: "missing value", args: []string{"--output"}},
		{name: "invalid value", args: []string{"--output=yaml"}},
		{name: "conflicting values", args: []string{"--json", "--output", "table"}},
	} {
		t.Run(test.name, func(t *testing.T) {
			if _, _, err := Extract(test.args); err == nil {
				t.Fatalf("Extract(%v) returned nil error", test.args)
			}
		})
	}
}

func TestExtractAcceptsEqualDuplicateFormats(t *testing.T) {
	format, args, err := Extract([]string{"--output=json", "--json", "version"})
	if err != nil {
		t.Fatalf("Extract: %v", err)
	}
	if format != FormatJSON || strings.Join(args, " ") != "version" {
		t.Fatalf("Extract = (%q, %q), want (json, version)", format, strings.Join(args, " "))
	}
}

func TestRenderJSONPreservesJSONValues(t *testing.T) {
	var got bytes.Buffer
	rows := Rows{JSON: map[string]any{"name": "symbrain", "count": 2}}
	if err := Render(&got, FormatJSON, rows); err != nil {
		t.Fatalf("Render: %v", err)
	}

	var decoded map[string]any
	if err := json.Unmarshal(got.Bytes(), &decoded); err != nil {
		t.Fatalf("output is not JSON: %v", err)
	}
	if decoded["name"] != "symbrain" || decoded["count"] != float64(2) {
		t.Fatalf("decoded = %v, want original values", decoded)
	}
}

func TestRenderTableUsesTableFunction(t *testing.T) {
	var got bytes.Buffer
	if err := Render(&got, FormatTable, Rows{Table: func(w io.Writer) error {
		_, err := w.Write([]byte("table output\n"))
		return err
	}}); err != nil {
		t.Fatalf("Render: %v", err)
	}
	if got.String() != "table output\n" {
		t.Fatalf("output = %q, want table output", got.String())
	}
}

func TestRenderTableWritesSimpleValuesAndAllowsEmptyRows(t *testing.T) {
	var got bytes.Buffer
	if err := Render(&got, FormatTable, "table output"); err != nil {
		t.Fatalf("Render simple table: %v", err)
	}
	if err := Render(&got, FormatTable, Rows{}); err != nil {
		t.Fatalf("Render empty rows: %v", err)
	}
	if got.String() != "table output\n" {
		t.Fatalf("output = %q, want simple value only", got.String())
	}
}

func TestRenderRejectsUnsupportedFormat(t *testing.T) {
	if err := Render(io.Discard, Format("yaml"), nil); err == nil {
		t.Fatal("Render(yaml) returned nil error")
	}
}

type errorWriter struct{}

func (errorWriter) Write([]byte) (int, error) {
	return 0, io.ErrClosedPipe
}

func TestRenderPropagatesWriterErrors(t *testing.T) {
	if err := Render(errorWriter{}, FormatTable, "value"); err == nil {
		t.Fatal("Render() returned nil error for a failing writer")
	}
}
