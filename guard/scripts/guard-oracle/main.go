package main

import (
	"bytes"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"
)

type oracleCase struct {
	ID         string          `json:"id"`
	Kind       string          `json:"kind"`
	Input      json.RawMessage `json:"input"`
	Success    bool            `json:"success"`
	OutputJSON string          `json:"output_json,omitempty"`
	Error      string          `json:"error,omitempty"`
}

type oracleSuite struct {
	Cases []oracleCase `json:"cases"`
}

func encoded(value any) string {
	data, err := json.Marshal(value)
	if err != nil {
		panic(err)
	}
	return string(data)
}

func input(value any) json.RawMessage {
	return json.RawMessage(encoded(value))
}

func successCase(id, kind string, in, out any) oracleCase {
	return oracleCase{ID: id, Kind: kind, Input: input(in), Success: true, OutputJSON: encoded(out)}
}

func errorCase(id, kind string, in any, err error) oracleCase {
	return oracleCase{ID: id, Kind: kind, Input: input(in), Success: false, Error: err.Error()}
}

func main() {
	check := flag.Bool("check", false, "fail if generated output does not match existing file")
	output := flag.String("output", "rust/symbrain-guard-core/tests/fixtures/oracle_expectations.json", "output expectations path")
	flag.Parse()

	suite := buildSuite()
	data, err := json.MarshalIndent(suite, "", "  ")
	if err != nil {
		fmt.Fprintf(os.Stderr, "json marshal: %v\n", err)
		os.Exit(1)
	}
	data = append(data, '\n')
	if *check {
		existing, readErr := os.ReadFile(*output)
		if readErr != nil {
			fmt.Fprintf(os.Stderr, "read %s: %v\n", *output, readErr)
			os.Exit(1)
		}
		if !bytes.Equal(existing, data) {
			fmt.Fprintf(os.Stderr, "%s is out of date; run go run ./guard/scripts/guard-oracle\n", *output)
			os.Exit(1)
		}
		fmt.Printf("PASS: guard oracle deterministic check passed (0 drift on %d cases)\n", len(suite.Cases))
		return
	}
	if err := os.MkdirAll(filepath.Dir(*output), 0o755); err != nil {
		fmt.Fprintf(os.Stderr, "mkdir %s: %v\n", filepath.Dir(*output), err)
		os.Exit(1)
	}
	if err := os.WriteFile(*output, data, 0o644); err != nil {
		fmt.Fprintf(os.Stderr, "write %s: %v\n", *output, err)
		os.Exit(1)
	}
	fmt.Printf("Wrote %s (%d cases)\n", *output, len(suite.Cases))
}
