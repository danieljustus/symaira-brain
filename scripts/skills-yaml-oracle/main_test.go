package main

import (
	"bytes"
	"encoding/json"
	"math"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

func TestActualGoCorpusAndMutationRejection(t *testing.T) {
	_, source, _, _ := runtime.Caller(0)
	root := filepath.Join(filepath.Dir(source), "../../rust/symbrain-skills")
	input, err := os.ReadFile(filepath.Join(root, "tests/fixtures/yaml835-documents-input.jsonl"))
	if err != nil {
		t.Fatal(err)
	}
	var first, second bytes.Buffer
	if err := capture(bytes.NewReader(input), &first); err != nil {
		t.Fatal(err)
	}
	if err := capture(bytes.NewReader(input), &second); err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(first.Bytes(), second.Bytes()) {
		t.Fatal("nondeterministic actual-Go output")
	}
	if err := checkGenerated(filepath.Join(root, "tests/fixtures/yaml835-documents-go.jsonl"), first.Bytes()); err != nil {
		t.Fatal(err)
	}
	lines := bytes.Split(bytes.TrimSuffix(first.Bytes(), []byte("\n")), []byte("\n"))
	if len(lines) != 38 {
		t.Fatalf("executed %d cases, want 38", len(lines))
	}
	var control struct {
		Types map[string]string `json:"input_number_types"`
	}
	if err := json.Unmarshal(lines[28], &control); err != nil {
		t.Fatal(err)
	}
	if control.Types["/metadata/values/1"] != "f64:8000000000000000" {
		t.Fatal("negative zero was not negative")
	}
	path := filepath.Join(t.TempDir(), "changed.jsonl")
	mutation := bytes.Replace(first.Bytes(), []byte("body-empty"), []byte("changed-id"), 1)
	if err := os.WriteFile(path, mutation, 0600); err != nil {
		t.Fatal(err)
	}
	if err := checkGenerated(path, first.Bytes()); err == nil || !strings.Contains(err.Error(), "oracle drift") {
		t.Fatalf("changed expectation not rejected: %v", err)
	}
	after, err := os.ReadFile(path)
	if err != nil || !bytes.Equal(after, mutation) {
		t.Fatal("check mode rewrote fixture")
	}
	// Every Go Unicode L/Nd range is regenerated through the production generator.
	var ranges bytes.Buffer
	if err := unicodeRanges(&ranges); err != nil {
		t.Fatal(err)
	}
	if err := checkGenerated(filepath.Join(root, "src/yaml_key_ranges.rs"), ranges.Bytes()); err != nil {
		t.Fatal(err)
	}
	t.Log("38 actual-Go documents; deterministic bytes; mutation rejected; pinned Unicode ranges checked")
}

func TestJSONNumberRuntimeKinds(t *testing.T) {
	for _, tc := range []struct{ text, kind string }{
		{"7", "i64"}, {"18446744073709551615", "u64"}, {"1.0", "f64:3ff0000000000000"}, {"-0.0", "f64:8000000000000000"},
		{"-0", "f64:8000000000000000"},
	} {
		t.Run(tc.text, func(t *testing.T) {
			value, err := adapt(json.Number(tc.text))
			if err != nil {
				t.Fatal(err)
			}
			types := map[string]string{}
			numberTypes(value, "/", types)
			if types["/"] != tc.kind {
				t.Fatalf("runtime kind %s", types["/"])
			}
			if tc.text == "-0.0" && !math.Signbit(value.(float64)) {
				t.Fatal("negative zero")
			}
		})
	}
}

func TestMalformedOracleInputFailsClosed(t *testing.T) {
	valid := `{"id":"one","frontmatter":{"name":"probe","description":"text"}}` + "\n"
	for _, input := range []string{"", valid + valid, strings.TrimSpace(valid) + " {}\n", `{"id":""}` + "\n", `{"id":"one","body_hex":"ffz"}` + "\n"} {
		var output bytes.Buffer
		if err := capture(strings.NewReader(input), &output); err == nil {
			t.Fatalf("accepted invalid input %q", input)
		}
	}
}
