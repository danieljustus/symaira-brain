package main

import (
	"bytes"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"

	"github.com/danieljustus/symaira-brain/internal/catalog"
	"github.com/danieljustus/symaira-brain/internal/policy"
)

type serverInput struct {
	Server string         `json:"server"`
	Tools  []catalog.Tool `json:"tools"`
	Report *policy.Report `json:"report"`
}

type testCase struct {
	ID      string        `json:"id"`
	Servers []serverInput `json:"servers"`
}

type expectation struct {
	ID      string          `json:"id"`
	Servers []serverInput   `json:"servers"`
	Success bool            `json:"success"`
	Entries []catalog.Entry `json:"entries,omitempty"`
	Names   []string        `json:"names,omitempty"`
	Error   string          `json:"error,omitempty"`
}

type suite struct {
	Cases []expectation `json:"cases"`
}

func boolPtr(value bool) *bool { return &value }

func cases() []testCase {
	return []testCase{
		{
			ID: "mixed-routing-metadata",
			Servers: []serverInput{
				{
					Server: "vault",
					Tools: []catalog.Tool{
						{Name: "health", Description: "health check"},
						{Name: "get_entry", Description: "fetch", InputSchema: json.RawMessage(`{"type":"object","properties":{"id":{"type":"string"}}}`)},
						{Name: "future_tool", Description: "unknown"},
					},
					Report: &policy.Report{Server: "vault", Enabled: true, Mode: "request_only", Exposed: []string{"health"}, Hidden: []string{"get_entry"}, Unknown: []string{"future_tool"}},
				},
				{
					Server: "zotero",
					Tools:  []catalog.Tool{{Name: "search", Description: "search library", Annotations: &catalog.ToolAnnotations{Title: "Search", ReadOnlyHint: boolPtr(true), DestructiveHint: boolPtr(false)}}},
					Report: &policy.Report{Server: "zotero", Enabled: true, Exposed: []string{"search"}, Hidden: []string{}, Unknown: []string{}, Exposures: map[string]policy.ToolExposure{"search": {Class: "read", Source: policy.ExposureSourceReadOnlyHint}}},
				},
			},
		},
		{
			ID: "already-prefixed-and-exact-prefix-edge",
			Servers: []serverInput{{
				Server: "vault",
				Tools:  []catalog.Tool{{Name: "memory_search"}, {Name: "entity_list"}, {Name: "graph_neighbors"}, {Name: "vault_"}},
				Report: &policy.Report{Server: "vault", Enabled: true, Mode: "full", Exposed: []string{"memory_search", "entity_list", "graph_neighbors", "vault_"}, Hidden: []string{}, Unknown: []string{}},
			}},
		},
		{
			ID: "post-namespace-collision",
			Servers: []serverInput{
				{Server: "vault", Tools: []catalog.Tool{{Name: "vault_shared_tool"}}, Report: &policy.Report{Server: "vault", Enabled: true, Mode: "full", Exposed: []string{"vault_shared_tool"}, Hidden: []string{}, Unknown: []string{}}},
				{Server: "memory", Tools: []catalog.Tool{{Name: "vault_shared_tool"}}, Report: &policy.Report{Server: "memory", Enabled: true, Mode: "read_write", Exposed: []string{"vault_shared_tool"}, Hidden: []string{}, Unknown: []string{}}},
			},
		},
	}
}

func generate() suite {
	result := suite{}
	for _, tc := range cases() {
		inputs := make([]catalog.ServerTools, len(tc.Servers))
		for i, server := range tc.Servers {
			inputs[i] = catalog.ServerTools{Server: server.Server, Tools: server.Tools, Report: server.Report}
		}
		built, err := catalog.Build(inputs)
		expected := expectation{ID: tc.ID, Servers: tc.Servers, Success: err == nil}
		if err != nil {
			expected.Error = err.Error()
		} else {
			expected.Entries = built.All()
			expected.Names = built.Names()
		}
		result.Cases = append(result.Cases, expected)
	}
	return result
}

func main() {
	check := flag.Bool("check", false, "fail if generated output does not match existing file")
	output := flag.String("output", "rust/symbrain-catalog/tests/fixtures/oracle_expectations.json", "output expectations path")
	flag.Parse()

	data, err := json.MarshalIndent(generate(), "", "  ")
	if err != nil {
		fmt.Fprintf(os.Stderr, "json marshal: %v\n", err)
		os.Exit(1)
	}
	data = append(data, '\n')
	if *check {
		existing, err := os.ReadFile(*output)
		if err != nil {
			fmt.Fprintf(os.Stderr, "read %s: %v\n", *output, err)
			os.Exit(1)
		}
		if !bytes.Equal(existing, data) {
			fmt.Fprintf(os.Stderr, "%s is out of date; run go run ./scripts/catalog-oracle\n", *output)
			os.Exit(1)
		}
		fmt.Println("PASS: catalog oracle deterministic check passed (0 drift on 3 cases)")
		return
	}
	if err := os.MkdirAll(filepath.Dir(*output), 0o755); err != nil {
		fmt.Fprintf(os.Stderr, "mkdir: %v\n", err)
		os.Exit(1)
	}
	if err := os.WriteFile(*output, data, 0o644); err != nil {
		fmt.Fprintf(os.Stderr, "write: %v\n", err)
		os.Exit(1)
	}
	fmt.Printf("Wrote %s (3 cases)\n", *output)
}
