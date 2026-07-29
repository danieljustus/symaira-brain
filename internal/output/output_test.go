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

func TestParseAcceptsValidFormats(t *testing.T) {
	for _, test := range []struct {
		name     string
		explicit string
		want     Format
	}{
		{name: "empty", explicit: "", want: FormatTable},
		{name: "table", explicit: "table", want: FormatTable},
		{name: "json", explicit: "json", want: FormatJSON},
		{name: "case insensitive", explicit: "JSON", want: FormatJSON},
	} {
		t.Run(test.name, func(t *testing.T) {
			got, err := Parse(test.explicit)
			if err != nil {
				t.Fatalf("Parse(%q) unexpected error: %v", test.explicit, err)
			}
			if got != test.want {
				t.Fatalf("Parse(%q) = %q, want %q", test.explicit, got, test.want)
			}
		})
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

func TestRenderNilTableReturnsNil(t *testing.T) {
	var got bytes.Buffer
	if err := Render(&got, FormatTable, Rows{Table: nil}); err != nil {
		t.Fatalf("Render nil Table: %v", err)
	}
	if got.Len() != 0 {
		t.Fatalf("expected empty output for nil Table, got %q", got.String())
	}
}

func TestRenderBadFormatReturnsError(t *testing.T) {
	var got bytes.Buffer
	if err := Render(&got, Format("bad"), nil); err == nil {
		t.Fatal("Render with bad format returned nil error")
	}
}

func TestExtractOutputEqualsPrefix(t *testing.T) {
	format, args, err := Extract([]string{"--output=json", "version"})
	if err != nil {
		t.Fatalf("Extract --output=: %v", err)
	}
	if format != FormatJSON || strings.Join(args, " ") != "version" {
		t.Fatalf("Extract --output= = (%q, %q), want (json, version)", format, strings.Join(args, " "))
	}
}

func TestExtractConflictingFormatsReturnsError(t *testing.T) {
	_, _, err := Extract([]string{"--json", "--output", "table"})
	if err == nil {
		t.Fatal("Extract with conflicting formats returned nil error")
	}
}

func TestExtractMissingValueReturnsError(t *testing.T) {
	_, _, err := Extract([]string{"--output"})
	if err == nil {
		t.Fatal("Extract --output without value returned nil error")
	}
}
