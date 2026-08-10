package harness

import (
	"bytes"
	"encoding/json"
	"strings"
	"testing"
)

func TestMarshalJSONValue_AllOrderedJSONTypes(t *testing.T) {
	nested := newOrderedMap()
	nested.set("nested", "value")

	tests := []struct {
		name  string
		value any
		want  string
	}{
		{name: "null", value: nil, want: "null"},
		{name: "ordered object", value: nested, want: "{\n  \"nested\": \"value\"\n}"},
		{name: "empty array", value: []any{}, want: "[]"},
		{
			name:  "array with scalar values",
			value: []any{json.Number("12.50"), "text", true, false, nil},
			want:  "[\n  12.50,\n  \"text\",\n  true,\n  false,\n  null\n]",
		},
		{name: "number", value: json.Number("-3.14"), want: "-3.14"},
		{name: "string", value: "plain", want: "\"plain\""},
		{name: "true", value: true, want: "true"},
		{name: "false", value: false, want: "false"},
		{name: "fallback", value: 42, want: "42"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			var buf bytes.Buffer
			if err := marshalJSONValue(&buf, tt.value, "", "  "); err != nil {
				t.Fatalf("marshalJSONValue() error = %v", err)
			}
			if got := buf.String(); got != tt.want {
				t.Errorf("marshalJSONValue() = %q, want %q", got, tt.want)
			}
		})
	}
}

func TestMarshalJSONValue_UnsupportedFallbackReturnsError(t *testing.T) {
	var buf bytes.Buffer
	if err := marshalJSONValue(&buf, make(chan int), "", "  "); err == nil {
		t.Fatal("marshalJSONValue(channel) error = nil, want unsupported-type error")
	}
}

func TestWriteJSONString_EscapesAndPreservesUnicode(t *testing.T) {
	input := "quote\" slash\\ " + "\b\f\n\r\t" + string(rune(1)) + " <> & é€😀"
	var buf bytes.Buffer
	if err := writeJSONString(&buf, input); err != nil {
		t.Fatalf("writeJSONString() error = %v", err)
	}

	want := `"quote\" slash\\ \b\f\n\r\t\u0001 <> & é€😀"`
	if got := buf.String(); got != want {
		t.Errorf("writeJSONString() = %q, want %q", got, want)
	}
	if strings.Contains(buf.String(), `\u003c`) || strings.Contains(buf.String(), `\u003e`) || strings.Contains(buf.String(), `\u0026`) {
		t.Error("writeJSONString() HTML-escaped characters, want literal HTML characters")
	}
}

func TestOrderedMap_MarshalJSON_NilAndEmpty(t *testing.T) {
	tests := []struct {
		name string
		m    *orderedMap
	}{
		{name: "nil", m: nil},
		{name: "empty", m: newOrderedMap()},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			var buf bytes.Buffer
			if err := tt.m.marshalJSON(&buf, "", "  "); err != nil {
				t.Fatalf("marshalJSON() error = %v", err)
			}
			if got := buf.String(); got != "{}" {
				t.Errorf("marshalJSON() = %q, want %q", got, "{}")
			}
		})
	}
}
