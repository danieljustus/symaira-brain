package output

import (
	"bytes"
	"encoding/json"
	"os"
	"strings"
	"testing"

	"golang.org/x/term"
)

func TestNewReporter_Default(t *testing.T) {
	r := NewReporter("")
	if r == nil {
		t.Fatal("NewReporter(\"\") returned nil")
	}
	// Should default to table
	var buf bytes.Buffer
	err := r.Print(&buf, "hello")
	if err != nil {
		t.Fatalf("Print() error = %v", err)
	}
	if !strings.Contains(buf.String(), "hello") {
		t.Errorf("expected 'hello', got %q", buf.String())
	}
}

func TestNewReporter_Table(t *testing.T) {
	r := NewReporter("table")
	if r == nil {
		t.Fatal("NewReporter(\"table\") returned nil")
	}
}

func TestNewReporter_JSON(t *testing.T) {
	r := NewReporter("json")
	if r == nil {
		t.Fatal("NewReporter(\"json\") returned nil")
	}
}

func TestNewReporter_Unknown(t *testing.T) {
	r := NewReporter("unknown")
	if r == nil {
		t.Fatal("NewReporter(\"unknown\") returned nil")
	}
	// Should fall back to table
}

func TestJSONReporter_Print(t *testing.T) {
	r := NewReporter("json")
	result := map[string]any{
		"name":  "test",
		"count": 42,
	}
	var buf bytes.Buffer
	err := r.Print(&buf, result)
	if err != nil {
		t.Fatalf("Print() error = %v", err)
	}

	var decoded map[string]any
	if err := json.Unmarshal(buf.Bytes(), &decoded); err != nil {
		t.Fatalf("json.Unmarshal error = %v", err)
	}
	if decoded["name"] != "test" {
		t.Errorf("name = %v, want test", decoded["name"])
	}
	if decoded["count"] != float64(42) {
		t.Errorf("count = %v, want 42", decoded["count"])
	}
}

func TestTableReporter_PrintString(t *testing.T) {
	r := NewReporter("table")
	var buf bytes.Buffer
	err := r.Print(&buf, "scan complete")
	if err != nil {
		t.Fatalf("Print() error = %v", err)
	}
	out := buf.String()
	if !strings.Contains(out, "scan complete") {
		t.Errorf("expected 'scan complete', got %q", out)
	}
}

func TestTableReporter_PrintStruct(t *testing.T) {
	r := NewReporter("table")
	result := struct {
		Name string `json:"name"`
	}{Name: "test"}
	var buf bytes.Buffer
	err := r.Print(&buf, result)
	if err != nil {
		t.Fatalf("Print() error = %v", err)
	}
	if buf.Len() == 0 {
		t.Error("expected non-empty output for struct")
	}
}

func TestFormatNames(t *testing.T) {
	names := FormatNames()
	if len(names) < 2 {
		t.Errorf("expected at least 2 formats, got %d", len(names))
	}
	hasTable := false
	hasJSON := false
	for _, n := range names {
		if n == "table" {
			hasTable = true
		}
		if n == "json" {
			hasJSON = true
		}
	}
	if !hasTable || !hasJSON {
		t.Error("FormatNames() missing table or json")
	}
}

func TestResolve_Explicit(t *testing.T) {
	if got := Resolve("json"); got != "json" {
		t.Errorf("Resolve(\"json\") = %q, want json", got)
	}
	if got := Resolve("table"); got != "table" {
		t.Errorf("Resolve(\"table\") = %q, want table", got)
	}
}

func TestResolve_EmptyOnTerminal(t *testing.T) {
	// Simulate terminal
	SetTerminalCheck(func(*os.File) bool { return true })
	defer SetTerminalCheck(func(w *os.File) bool { return term.IsTerminal(int(w.Fd())) })

	if got := Resolve(""); got != "table" {
		t.Errorf("Resolve(\"\") on terminal = %q, want table", got)
	}
}

func TestResolve_EmptyOnPipe(t *testing.T) {
	// Simulate pipe (non-terminal)
	SetTerminalCheck(func(*os.File) bool { return false })
	defer SetTerminalCheck(func(w *os.File) bool { return term.IsTerminal(int(w.Fd())) })

	if got := Resolve(""); got != "json" {
		t.Errorf("Resolve(\"\") on pipe = %q, want json", got)
	}
}

func TestNewReporter_EmptyUsesResolve(t *testing.T) {
	// On a terminal, empty format should give table reporter
	SetTerminalCheck(func(*os.File) bool { return true })
	defer SetTerminalCheck(func(w *os.File) bool { return term.IsTerminal(int(w.Fd())) })

	r := NewReporter("")
	var buf bytes.Buffer
	err := r.Print(&buf, "hello")
	if err != nil {
		t.Fatalf("Print() error = %v", err)
	}
	// table reporter just prints strings directly
	if !strings.Contains(buf.String(), "hello") {
		t.Errorf("expected 'hello', got %q", buf.String())
	}
}
