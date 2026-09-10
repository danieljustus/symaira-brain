// Command usage-oracle freezes the source-owned Usage/get_ai_usage graph and
// deterministic provider cases by exercising the production Go providers
// against fixture transport. It never resolves credentials or contacts a live
// service.
package main

import (
	"bytes"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"

	"github.com/danieljustus/symaira-brain/internal/usage"
)

func loadFixtures(dir string) (map[string][]byte, error) {
	names := map[string]string{
		"claude": "claude-oauth-usage.json", "codex": "codex-wham-usage.json",
		"copilot": "copilot-user.json", "cursor": "cursor-usage-summary.json",
		"kimi": "kimi-api-usages.json", "moonshot": "moonshot-balance-ai.json",
		"nous": "nous-account.json", "opencode": "opencode-subscription-json.txt",
		"openrouter": "openrouter-credits.json", "antigravity": "antigravity-quota-summary.json",
	}
	out := make(map[string][]byte, len(names))
	for id, name := range names {
		data, err := os.ReadFile(filepath.Join(dir, name))
		if err != nil {
			return nil, fmt.Errorf("read %s fixture: %w", id, err)
		}
		out[id] = data
	}
	return out, nil
}

func writeJSON(path string, value any) error {
	data, err := json.MarshalIndent(value, "", "  ")
	if err != nil {
		return err
	}
	data = append(data, '\n')
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return err
	}
	return os.WriteFile(path, data, 0o644)
}

func checkJSON(path string, value any) error {
	data, err := json.MarshalIndent(value, "", "  ")
	if err != nil {
		return err
	}
	data = append(data, '\n')
	existing, err := os.ReadFile(path)
	if err != nil || !bytes.Equal(existing, data) {
		return fmt.Errorf("%s is out of date; run go run ./scripts/usage-oracle", path)
	}
	return nil
}

func main() {
	check := flag.Bool("check", false, "fail if generated fixtures differ")
	output := flag.String("output", "rust/symbrain-usage/tests/fixtures/provider_graph.json", "provider graph path")
	casesOutput := flag.String("cases-output", "rust/symbrain-usage/tests/fixtures/provider_cases.json", "provider cases path")
	flag.Parse()

	fixtures, err := loadFixtures(filepath.Join("internal", "usage", "testdata"))
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	cases, err := usage.BuildOracleFixture(fixtures)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	graph := struct {
		SchemaVersion int                      `json:"schema_version"`
		Providers     []usage.ContractProvider `json:"providers"`
	}{SchemaVersion: usage.ReportSchemaVersion, Providers: usage.ContractProviders()}
	caseWire := struct {
		SchemaVersion int                    `json:"schema_version"`
		Providers     []usage.OracleProvider `json:"providers"`
	}{SchemaVersion: usage.ReportSchemaVersion, Providers: cases.Providers}

	if *check {
		if err := checkJSON(*output, graph); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		if err := checkJSON(*casesOutput, caseWire); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		fmt.Printf("PASS: usage oracle deterministic check passed (%d providers, %d cases)\n", len(graph.Providers), len(caseWire.Providers))
		return
	}
	if err := writeJSON(*output, graph); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err := writeJSON(*casesOutput, caseWire); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	fmt.Printf("Wrote %s and %s (%d providers)\n", *output, *casesOutput, len(graph.Providers))
}
