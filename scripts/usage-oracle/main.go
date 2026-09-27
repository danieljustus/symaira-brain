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

const copilotOracleToken = "oracle-only-invalid-copilot"
const openRouterOracleToken = "oracle-only-invalid-openrouter"
const moonshotOracleToken = "oracle-only-invalid-moonshot"

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
	copilotReportOutput := flag.String("copilot-report-output", "rust/symbrain-usage/tests/fixtures/copilot_authenticated_report.json", "authenticated Copilot report path")
	openRouterReportOutput := flag.String("openrouter-report-output", "rust/symbrain-usage/tests/fixtures/openrouter_authenticated_report.json", "authenticated OpenRouter report path")
	moonshotReportOutput := flag.String("moonshot-report-output", "rust/symbrain-usage/tests/fixtures/moonshot_authenticated_report.json", "authenticated Moonshot report path")
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
	copilotReport, err := buildCopilotAuthenticatedReport(fixtures["copilot"])
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	openRouterReport, err := buildOpenRouterAuthenticatedReport(fixtures["openrouter"])
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	moonshotReport, err := buildMoonshotAuthenticatedReport(fixtures["moonshot"])
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}

	if *check {
		if err := checkJSON(*output, graph); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		if err := checkJSON(*casesOutput, caseWire); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		if err := checkJSON(*copilotReportOutput, copilotReport); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		if err := checkJSON(*openRouterReportOutput, openRouterReport); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		if err := checkJSON(*moonshotReportOutput, moonshotReport); err != nil {
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
	if err := writeJSON(*copilotReportOutput, copilotReport); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err := writeJSON(*openRouterReportOutput, openRouterReport); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err := writeJSON(*moonshotReportOutput, moonshotReport); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	fmt.Printf("Wrote %s, %s, %s, %s, and %s (%d providers)\n", *output, *casesOutput, *copilotReportOutput, *openRouterReportOutput, *moonshotReportOutput, len(graph.Providers))
}

func buildCopilotAuthenticatedReport(body []byte) (usage.Report, error) {
	return buildAuthenticatedProviderReport("copilot", copilotOracleToken, body)
}

func buildOpenRouterAuthenticatedReport(body []byte) (usage.Report, error) {
	return buildAuthenticatedProviderReport("openrouter", openRouterOracleToken, body)
}

func buildMoonshotAuthenticatedReport(body []byte) (usage.Report, error) {
	return buildAuthenticatedProviderReport("moonshot", moonshotOracleToken, body)
}

func buildAuthenticatedProviderReport(provider, token string, body []byte) (usage.Report, error) {
	// Keep the full ten-provider production graph deterministic without
	// inheriting a developer's credential environment or home directory.
	credentialEnv := []string{
		"HOME", "CODEX_HOME", "HERMES_HOME", "KIMI_CODE_HOME",
		"ANTHROPIC_ADMIN_KEY", "ANTHROPIC_OAUTH_TOKEN", "CODEX_ACCESS_TOKEN",
		"COPILOT_ACCESS_TOKEN", "CURSOR_COOKIE", "KIMI_CODE_API_KEY",
		"KIMI_AUTH_TOKEN", "KIMI_CODE_BASE_URL", "MOONSHOT_API_KEY",
		"MOONSHOT_REGION", "NOUS_PORTAL_ACCESS_TOKEN", "HERMES_PORTAL_BASE_URL",
		"OPENCODE_COOKIE", "OPENCODE_WORKSPACE_ID", "OPENROUTER_API_KEY",
		"OPENROUTER_API_URL",
	}
	previous := make(map[string]string, len(credentialEnv))
	existed := make(map[string]bool, len(credentialEnv))
	for _, name := range credentialEnv {
		previous[name], existed[name] = os.LookupEnv(name)
	}
	restore := func() error {
		for _, name := range credentialEnv {
			var err error
			if existed[name] {
				err = os.Setenv(name, previous[name])
			} else {
				err = os.Unsetenv(name)
			}
			if err != nil {
				return fmt.Errorf("restore %s: %w", name, err)
			}
		}
		return nil
	}
	for _, name := range credentialEnv {
		if err := os.Unsetenv(name); err != nil {
			_ = restore()
			return usage.Report{}, fmt.Errorf("clear %s for usage oracle: %w", name, err)
		}
	}
	home, err := os.MkdirTemp("", "symbrain-usage-oracle-home-")
	if err != nil {
		_ = restore()
		return usage.Report{}, fmt.Errorf("create isolated usage oracle home: %w", err)
	}
	if err := os.Setenv("HOME", home); err != nil {
		_ = os.RemoveAll(home)
		_ = restore()
		return usage.Report{}, fmt.Errorf("set isolated usage oracle home: %w", err)
	}
	envName := map[string]string{
		"copilot":    "COPILOT_ACCESS_TOKEN",
		"moonshot":   "MOONSHOT_API_KEY",
		"openrouter": "OPENROUTER_API_KEY",
	}[provider]
	if envName == "" {
		_ = os.RemoveAll(home)
		_ = restore()
		return usage.Report{}, fmt.Errorf("unsupported authenticated usage oracle provider %q", provider)
	}
	if err := os.Setenv(envName, token); err != nil {
		_ = os.RemoveAll(home)
		_ = restore()
		return usage.Report{}, fmt.Errorf("set synthetic %s fixture env: %w", provider, err)
	}
	var report usage.Report
	var buildErr error
	switch provider {
	case "copilot":
		report, buildErr = usage.BuildCopilotAuthenticatedReportOracle(body)
	case "openrouter":
		report, buildErr = usage.BuildOpenRouterAuthenticatedReportOracle(body)
	case "moonshot":
		report, buildErr = usage.BuildMoonshotAuthenticatedReportOracle(body)
	}
	removeErr := os.RemoveAll(home)
	restoreErr := restore()
	if buildErr != nil {
		return usage.Report{}, buildErr
	}
	if removeErr != nil {
		return usage.Report{}, fmt.Errorf("remove isolated usage oracle home: %w", removeErr)
	}
	if restoreErr != nil {
		return usage.Report{}, fmt.Errorf("restore usage oracle environment: %w", restoreErr)
	}
	return report, nil
}
