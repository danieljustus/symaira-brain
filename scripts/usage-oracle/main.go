// Command usage-oracle freezes the source-owned Usage/get_ai_usage graph and
// deterministic provider cases by exercising the production Go providers
// against fixture transport. It never resolves credentials or contacts a live
// service.
package main

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"time"

	"github.com/danieljustus/symaira-brain/internal/usage"
)

const copilotOracleToken = "oracle-only-invalid-copilot"
const copilotFileOracleToken = "oracle-only-invalid-copilot-file"
const claudeAdminOracleToken = "oracle-only-invalid-claude-admin"
const claudeOAuthOracleToken = "oracle-only-invalid-claude-oauth"
const claudeOAuthFileOracleToken = "oracle-only-invalid-claude-oauth-file"
const codexOracleToken = "oracle-only-invalid-codex"
const cursorOracleToken = "oracle-only-invalid-cursor"
const kimiOracleToken = "oracle-only-invalid-kimi"
const nousOracleToken = "oracle-only-invalid-nous"
const openRouterOracleToken = "oracle-only-invalid-openrouter"
const moonshotOracleToken = "oracle-only-invalid-moonshot"
const openCodeOracleToken = "oracle-only-invalid-opencode"
const codexFileOracleToken = "oracle-only-invalid-codex-file"

func loadFixtures(dir string) (map[string][]byte, error) {
	names := map[string]string{
		"claude": "claude-oauth-usage.json", "claude-admin": "claude-admin-cost.json", "codex": "codex-wham-usage.json",
		"copilot": "copilot-user.json", "cursor": "cursor-usage-summary.json",
		"kimi": "kimi-api-usages.json", "kimi-fallback": "kimi-fallback-api-usages.json", "kimi-stable": "kimi-fallback-api-usages.json", "kimi-web": "kimi-web-usages.json", "kimi-web-stable": "kimi-fallback-web-usages.json", "moonshot": "moonshot-balance-ai.json", "moonshot-cn": "moonshot-balance-cn.json",
		"nous": "nous-account.json", "opencode": "opencode-subscription-json.txt", "opencode-workspaces": "opencode-workspaces.txt",
		"opencode-stable":         "opencode-subscription-no-reset.json",
		"openrouter":              "openrouter-credits.json",
		"antigravity":             "antigravity-quota-summary.json",
		"antigravity-user-status": "antigravity-user-status.json",
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
	copilotFileReportOutput := flag.String("copilot-file-report-output", "rust/symbrain-usage/tests/fixtures/copilot_file_authenticated_report.json", "authenticated Copilot file report path")
	copilotFileTokenOutput := flag.String("copilot-file-token-output", "rust/symbrain-usage/tests/fixtures/copilot_file_token_oracle.json", "Copilot credential file parser cases")
	nousFileTokenOutput := flag.String("nous-file-token-output", "rust/symbrain-usage/tests/fixtures/nous_file_token_oracle.json", "Nous auth.json parser cases")
	kimiFileTokenOutput := flag.String("kimi-file-token-output", "rust/symbrain-usage/tests/fixtures/kimi_file_token_oracle.json", "Kimi CLI credential parser cases")
	claudeAdminReportOutput := flag.String("claude-admin-report-output", "rust/symbrain-usage/tests/fixtures/claude_admin_authenticated_report.json", "authenticated Claude Admin report path")
	claudeOAuthReportOutput := flag.String("claude-oauth-report-output", "rust/symbrain-usage/tests/fixtures/claude_oauth_authenticated_report.json", "authenticated Claude OAuth report path")
	codexReportOutput := flag.String("codex-report-output", "rust/symbrain-usage/tests/fixtures/codex_authenticated_report.json", "authenticated Codex report path")
	codexFileReportOutput := flag.String("codex-file-report-output", "rust/symbrain-usage/tests/fixtures/codex_file_authenticated_report.json", "authenticated Codex auth.json report path")
	claudeFileTokenOutput := flag.String("claude-file-token-output", "rust/symbrain-usage/tests/fixtures/claude_file_token_oracle.json", "Claude credentials file parser cases")
	openRouterReportOutput := flag.String("openrouter-report-output", "rust/symbrain-usage/tests/fixtures/openrouter_authenticated_report.json", "authenticated OpenRouter report path")
	moonshotReportOutput := flag.String("moonshot-report-output", "rust/symbrain-usage/tests/fixtures/moonshot_authenticated_report.json", "authenticated Moonshot report path")
	cursorReportOutput := flag.String("cursor-report-output", "rust/symbrain-usage/tests/fixtures/cursor_authenticated_report.json", "authenticated Cursor report path")
	kimiReportOutput := flag.String("kimi-report-output", "rust/symbrain-usage/tests/fixtures/kimi_authenticated_report.json", "authenticated Kimi report path")
	nousReportOutput := flag.String("nous-report-output", "rust/symbrain-usage/tests/fixtures/nous_authenticated_report.json", "authenticated Nous report path")
	nousFileReportOutput := flag.String("nous-file-report-output", "rust/symbrain-usage/tests/fixtures/nous_file_authenticated_report.json", "authenticated Nous auth.json report path")
	openCodeReportOutput := flag.String("opencode-report-output", "rust/symbrain-usage/tests/fixtures/opencode_authenticated_report.json", "authenticated OpenCode report path")
	combinedReportOutput := flag.String("combined-native-report-output", "rust/symbrain-usage/tests/fixtures/combined_native_authenticated_report.json", "combined configured-provider report path")
	kimiFallbackReportOutput := flag.String("kimi-fallback-report-output", "rust/symbrain-usage/tests/fixtures/kimi_fallback_authenticated_report.json", "Kimi API/CLI/web fallback report path")
	nousPrecedenceReportOutput := flag.String("nous-precedence-report-output", "rust/symbrain-usage/tests/fixtures/nous_env_file_precedence_report.json", "Nous environment-over-file precedence report path")
	antigravityReportOutput := flag.String("antigravity-report-output", "rust/symbrain-usage/tests/fixtures/antigravity_authenticated_report.json", "Antigravity local probe reports from synthetic observations")
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
	copilotFileReport, err := buildCopilotFileAuthenticatedReport(fixtures["copilot"])
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	copilotFileTokenOracle, err := buildCopilotFileTokenOracle()
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	nousFileTokenOracle, err := buildNousFileTokenOracle()
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	kimiFileTokenOracle, err := buildKimiFileTokenOracle()
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	claudeAdminReport, err := buildClaudeAdminAuthenticatedReport(fixtures["claude-admin"])
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	claudeOAuthReport, err := buildClaudeOAuthAuthenticatedReport(fixtures["claude"])
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	claudeFileTokenOracle, err := buildClaudeFileTokenOracle()
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	codexReport, err := buildCodexAuthenticatedReport(fixtures["codex"])
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	codexFileReport, err := buildCodexFileAuthenticatedReport(fixtures["codex"])
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
	cursorReport, err := buildCursorAuthenticatedReport(fixtures["cursor"])
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	kimiReport, err := buildKimiAuthenticatedReport(fixtures["kimi"])
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	nousReport, err := buildNousAuthenticatedReport(fixtures["nous"])
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	nousFileReport, err := buildNousFileAuthenticatedReport(fixtures["nous"])
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	openCodeReport, err := buildOpenCodeAuthenticatedReport(fixtures["opencode-workspaces"], fixtures["opencode"])
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	combinedNativeReport, err := usage.BuildCombinedNativeAuthenticatedReportOracle(fixtures)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	kimiFallbackReport, err := usage.BuildKimiFallbackAuthenticatedReportOracle(fixtures["kimi-fallback"], fixtures["kimi-web-stable"])
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	nousPrecedenceReport, err := usage.BuildNousEnvironmentPrecedenceReportOracle(fixtures["nous"])
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	antigravityReport, err := usage.BuildAntigravityAuthenticatedReportOracle(fixtures)
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
		if err := checkJSON(*copilotFileReportOutput, copilotFileReport); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		if err := checkJSON(*copilotFileTokenOutput, copilotFileTokenOracle); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		if err := checkJSON(*nousFileTokenOutput, nousFileTokenOracle); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		if err := checkJSON(*kimiFileTokenOutput, kimiFileTokenOracle); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		if err := checkJSON(*claudeAdminReportOutput, claudeAdminReport); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		if err := checkJSON(*claudeOAuthReportOutput, claudeOAuthReport); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		if err := checkJSON(*claudeFileTokenOutput, claudeFileTokenOracle); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		if err := checkJSON(*codexReportOutput, codexReport); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		if err := checkJSON(*codexFileReportOutput, codexFileReport); err != nil {
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
		if err := checkJSON(*cursorReportOutput, cursorReport); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		if err := checkJSON(*kimiReportOutput, kimiReport); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		if err := checkJSON(*nousReportOutput, nousReport); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		if err := checkJSON(*nousFileReportOutput, nousFileReport); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		if err := checkJSON(*openCodeReportOutput, openCodeReport); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		if err := checkJSON(*combinedReportOutput, combinedNativeReport); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		if err := checkJSON(*kimiFallbackReportOutput, kimiFallbackReport); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		if err := checkJSON(*nousPrecedenceReportOutput, nousPrecedenceReport); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		if err := checkJSON(*antigravityReportOutput, antigravityReport); err != nil {
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
	if err := writeJSON(*copilotFileReportOutput, copilotFileReport); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err := writeJSON(*copilotFileTokenOutput, copilotFileTokenOracle); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err := writeJSON(*nousFileTokenOutput, nousFileTokenOracle); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err := writeJSON(*kimiFileTokenOutput, kimiFileTokenOracle); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err := writeJSON(*claudeAdminReportOutput, claudeAdminReport); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err := writeJSON(*claudeOAuthReportOutput, claudeOAuthReport); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err := writeJSON(*claudeFileTokenOutput, claudeFileTokenOracle); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err := writeJSON(*codexReportOutput, codexReport); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err := writeJSON(*codexFileReportOutput, codexFileReport); err != nil {
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
	if err := writeJSON(*cursorReportOutput, cursorReport); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err := writeJSON(*kimiReportOutput, kimiReport); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err := writeJSON(*nousReportOutput, nousReport); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err := writeJSON(*nousFileReportOutput, nousFileReport); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err := writeJSON(*openCodeReportOutput, openCodeReport); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err := writeJSON(*combinedReportOutput, combinedNativeReport); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err := writeJSON(*kimiFallbackReportOutput, kimiFallbackReport); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err := writeJSON(*nousPrecedenceReportOutput, nousPrecedenceReport); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err := writeJSON(*antigravityReportOutput, antigravityReport); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	fmt.Printf("Wrote usage graph, request cases, and authenticated provider reports (%d providers)\n", len(graph.Providers))
}

func buildCopilotAuthenticatedReport(body []byte) (usage.Report, error) {
	return buildAuthenticatedProviderReport("copilot", copilotOracleToken, body)
}

func buildClaudeAdminAuthenticatedReport(body []byte) (usage.Report, error) {
	return buildAuthenticatedProviderReport("claude-admin", claudeAdminOracleToken, body)
}

type claudeOAuthReportFixture struct {
	Success     usage.Report `json:"success"`
	FileSuccess usage.Report `json:"file_success"`
	Errors      []struct {
		Status int          `json:"status"`
		Report usage.Report `json:"report"`
	} `json:"errors"`
	FileErrors []struct {
		Status int          `json:"status"`
		Report usage.Report `json:"report"`
	} `json:"file_errors"`
}

type claudeFileTokenOracleFixture struct {
	SchemaVersion int                         `json:"schema_version"`
	Cases         []claudeFileTokenOracleCase `json:"cases"`
}

type claudeFileTokenOracleCase struct {
	ID             string   `json:"id"`
	Contents       string   `json:"contents"`
	Token          *string  `json:"token,omitempty"`
	PossibleTokens []string `json:"possible_tokens,omitempty"`
	SelectionRule  string   `json:"selection_rule,omitempty"`
}

type copilotFileTokenOracleFixture struct {
	SchemaVersion int                          `json:"schema_version"`
	Cases         []copilotFileTokenOracleCase `json:"cases"`
}

type copilotFileTokenOracleCase struct {
	ID             string   `json:"id"`
	AppsJSON       *string  `json:"apps_json,omitempty"`
	HostsJSON      *string  `json:"hosts_json,omitempty"`
	Token          *string  `json:"token,omitempty"`
	PossibleTokens []string `json:"possible_tokens,omitempty"`
	SelectionRule  string   `json:"selection_rule,omitempty"`
	RustCandidate  bool     `json:"rust_candidate"`
	NativeRoute    bool     `json:"native_route"`
}

type nousFileTokenOracleFixture struct {
	SchemaVersion int                       `json:"schema_version"`
	Cases         []nousFileTokenOracleCase `json:"cases"`
}

type nousFileTokenOracleCase struct {
	ID          string  `json:"id"`
	FilePresent bool    `json:"file_present"`
	Contents    *string `json:"contents,omitempty"`
	Token       string  `json:"token"`
	NativeRoute bool    `json:"native_route"`
}

type kimiFileTokenOracleFixture struct {
	SchemaVersion int                       `json:"schema_version"`
	Cases         []kimiFileTokenOracleCase `json:"cases"`
}

type kimiFileTokenOracleCase struct {
	ID          string  `json:"id"`
	FilePresent bool    `json:"file_present"`
	Contents    *string `json:"contents,omitempty"`
	Token       string  `json:"token"`
	NativeRoute bool    `json:"native_route"`
}

type codexFileReportFixture struct {
	Success usage.Report `json:"success"`
	Errors  []struct {
		Status int          `json:"status"`
		Report usage.Report `json:"report"`
	} `json:"errors"`
}

type copilotFileReportFixture struct {
	Success usage.Report `json:"success"`
	Errors  []struct {
		Status int          `json:"status"`
		Report usage.Report `json:"report"`
	} `json:"errors"`
}

type nousFileReportFixture struct {
	Success usage.Report `json:"success"`
	Errors  []struct {
		Status int          `json:"status"`
		Report usage.Report `json:"report"`
	} `json:"errors"`
}

func buildNousFileTokenOracle() (nousFileTokenOracleFixture, error) {
	file := func(value string) *string { return &value }
	jwt := func(payload string) string {
		return "header." + base64.RawURLEncoding.EncodeToString([]byte(payload)) + ".signature"
	}
	inputs := []struct {
		id       string
		contents *string
	}{
		{id: "canonical-plain-invoke-token", contents: file(`{"providers":[{"id":"nous","invoke_jwt":"synthetic-nous-token"}]}`)},
		{id: "case-insensitive-struct-fields", contents: file(`{"Providers":[{"ID":"nous","Invoke_JWT":"synthetic-nous-case-token"}]}`)},
		{id: "canonical-then-case-alias", contents: file(`{"providers":[{"id":"nous","access_token":"canonical-token","ACCESS_TOKEN":"alias-token"}]}`)},
		{id: "case-alias-then-canonical", contents: file(`{"providers":[{"id":"nous","ACCESS_TOKEN":"alias-token","access_token":"canonical-token"}]}`)},
		{id: "invoke-jwt-case-alias", contents: file(`{"providers":[{"id":"nous","invoke_jwt":"canonical-token","INVOKE_JWT":"alias-token"}]}`)},
		{id: "case-alias-providers-then-canonical", contents: file(`{"Providers":[{"id":"nous","access_token":"alias-token"}],"providers":[{"id":"nous","access_token":"canonical-token"}]}`)},
		{id: "canonical-providers-then-case-alias", contents: file(`{"providers":[{"id":"nous","access_token":"canonical-token"}],"Providers":[{"id":"nous","access_token":"alias-token"}]}`)},
		{id: "unknown-provider-field-with-token", contents: file(`{"providers":[{"id":"nous","access_token":"synthetic-nous-token","unknown_metadata":true}]}`)},
		{id: "missing-file"},
		{id: "empty-file", contents: file("")},
		{id: "malformed-json", contents: file(`{"providers":[`)},
		{id: "wrong-typed-token-invalidates-file", contents: file(`{"providers":[{"id":"nous","invoke_jwt":42}]}`)},
		{id: "duplicate-token-key-remains-go-only", contents: file(`{"providers":[{"id":"nous","access_token":"first","access_token":"last"}]}`)},
		{id: "jwt-shaped-token-remains-go-only", contents: file(`{"providers":[{"id":"nous","invoke_jwt":"a.b.c"}]}`)},
		{id: "canonical-future-jwt", contents: file(fmt.Sprintf(`{"version":1,"providers":[{"id":"nous","invoke_jwt":%q,"client_id":"hermes-cli"}]}`, jwt(`{"exp":4102444800}`)))},
		{id: "canonical-fractional-future-jwt", contents: file(fmt.Sprintf(`{"providers":[{"id":"nous","invoke_jwt":%q}]}`, jwt(`{"exp":4102444800.9,"sub":"synthetic"}`)))},
		{id: "jwt-exp-case-alias-after-canonical", contents: file(fmt.Sprintf(`{"providers":[{"id":"nous","invoke_jwt":%q}]}`, jwt(`{"exp":4102444800,"EXP":0}`)))},
		{id: "jwt-exp-case-alias-before-canonical", contents: file(fmt.Sprintf(`{"providers":[{"id":"nous","invoke_jwt":%q}]}`, jwt(`{"EXP":0,"exp":4102444800}`)))},
		{id: "jwt-duplicate-exp-invalid-before-valid", contents: file(fmt.Sprintf(`{"providers":[{"id":"nous","invoke_jwt":%q}]}`, jwt(`{"exp":"invalid","exp":4102444800}`)))},
		{id: "jwt-duplicate-exp-valid-before-invalid", contents: file(fmt.Sprintf(`{"providers":[{"id":"nous","invoke_jwt":%q}]}`, jwt(`{"exp":4102444800,"exp":"invalid"}`)))},
		{id: "expired-jwt", contents: file(fmt.Sprintf(`{"providers":[{"id":"nous","invoke_jwt":%q}]}`, jwt(`{"exp":1}`)))},
		{id: "case-insensitive-exp-is-go-only", contents: file(fmt.Sprintf(`{"providers":[{"id":"nous","invoke_jwt":%q}]}`, jwt(`{"EXP":4102444800}`)))},
		{id: "string-exp-is-invalid", contents: file(fmt.Sprintf(`{"providers":[{"id":"nous","invoke_jwt":%q}]}`, jwt(`{"exp":"4102444800"}`)))},
		{id: "malformed-jwt-payload-is-invalid", contents: file("{\"providers\":[{\"id\":\"nous\",\"invoke_jwt\":\"header.eyJleHA.signature\"}]}")},
		{id: "padded-jwt-payload-is-invalid", contents: file(`{"providers":[{"id":"nous","invoke_jwt":"header.eyJleHAiOjQxMDI0NDQ4MDB9=.signature"}]}`)},
		{id: "secret-reference-token-remains-go-only", contents: file(`{"providers":[{"id":"nous","access_token":"symvault://nous/token"}]}`)},
		{id: "unrelated-provider", contents: file(`{"providers":[{"id":"other","access_token":"other-token"}]}`)},
	}
	fixture := nousFileTokenOracleFixture{SchemaVersion: 1}
	native := map[string]bool{
		"canonical-plain-invoke-token":    true,
		"canonical-future-jwt":            true,
		"canonical-fractional-future-jwt": true,
	}
	for _, input := range inputs {
		token, err := usage.BuildNousFileTokenOracle(input.contents)
		if err != nil {
			return nousFileTokenOracleFixture{}, err
		}
		fixture.Cases = append(fixture.Cases, nousFileTokenOracleCase{
			ID: input.id, FilePresent: input.contents != nil, Contents: input.contents, Token: token, NativeRoute: native[input.id],
		})
	}
	return fixture, nil
}

func buildKimiFileTokenOracle() (kimiFileTokenOracleFixture, error) {
	file := func(value string) *string { return &value }
	inputs := []struct {
		id       string
		contents *string
	}{
		{id: "canonical-with-ignored-refresh-token", contents: file(`{"access_token":"synthetic-kimi-file-token","refresh_token":"unused-refresh"}`)},
		{id: "case-insensitive-struct-field", contents: file(`{"Access_Token":"synthetic-kimi-case-token"}`)},
		{id: "canonical-then-case-alias", contents: file(`{"access_token":"canonical-token","ACCESS_TOKEN":"alias-token"}`)},
		{id: "case-alias-then-canonical", contents: file(`{"ACCESS_TOKEN":"alias-token","access_token":"canonical-token"}`)},
		{id: "unknown-field-with-token", contents: file(`{"access_token":"synthetic-kimi-token","unknown_metadata":true}`)},
		{id: "duplicate-token-field", contents: file(`{"access_token":"first-token","access_token":"last-token"}`)},
		{id: "wrong-typed-token", contents: file(`{"access_token":42}`)},
		{id: "secret-reference-token", contents: file(`{"access_token":"symvault://kimi/token"}`)},
		{id: "missing-file"},
	}
	native := map[string]bool{"canonical-with-ignored-refresh-token": true}
	fixture := kimiFileTokenOracleFixture{SchemaVersion: 1}
	for _, input := range inputs {
		token, err := usage.BuildKimiFileTokenOracle(input.contents)
		if err != nil {
			return kimiFileTokenOracleFixture{}, err
		}
		fixture.Cases = append(fixture.Cases, kimiFileTokenOracleCase{
			ID: input.id, FilePresent: input.contents != nil, Contents: input.contents,
			Token: token, NativeRoute: native[input.id],
		})
	}
	return fixture, nil
}

func buildCopilotFileTokenOracle() (copilotFileTokenOracleFixture, error) {
	file := func(value string) *string { return &value }
	inputs := []struct {
		id             string
		apps           *string
		hosts          *string
		possibleTokens []string
		rustCandidate  bool
		nativeRoute    bool
	}{
		{
			id:            "single-app-entry-prefers-github-prefix",
			apps:          file(`{"github.com:Iv1.test":{"user":"dev","oauth_token":"apps-token"}}`),
			rustCandidate: true,
			nativeRoute:   true,
		},
		{
			id:            "apps-token-precedes-hosts-token",
			apps:          file(`{"enterprise":{"oauth_token":"apps-token"}}`),
			hosts:         file(`{"github.com:Iv1.test":{"oauth_token":"hosts-token"}}`),
			rustCandidate: true,
			nativeRoute:   true,
		},
		{
			id:            "single-nongithub-app-entry-uses-fallback-pass",
			apps:          file(`{"enterprise":{"oauth_token":"enterprise-token"}}`),
			rustCandidate: true,
			nativeRoute:   true,
		},
		{
			id:            "single-hosts-entry-when-apps-absent",
			hosts:         file(`{"github.com:Iv1.test":{"user":"dev","oauth_token":"hosts-token"}}`),
			rustCandidate: true,
			nativeRoute:   true,
		},
		{
			id:            "duplicate-root-key-last-value",
			apps:          file(`{"github.com:Iv1.test":{"oauth_token":"first-token"},"github.com:Iv1.test":{"oauth_token":"last-token"}}`),
			rustCandidate: true,
			nativeRoute:   true,
		},
		{
			id:          "alternate-case-token-field-is-go-readable",
			apps:        file(`{"github.com:Iv1.test":{"OAUTH_TOKEN":"case-token"}}`),
			nativeRoute: false,
		},
		{
			id:          "duplicate-token-fields-use-go-decode-order",
			apps:        file(`{"github.com:Iv1.test":{"oauth_token":"first-token","oauth_token":"last-token"}}`),
			nativeRoute: false,
		},
		{
			id:          "unknown-entry-metadata-is-go-readable",
			apps:        file(`{"github.com:Iv1.test":{"oauth_token":"apps-token","refresh_token":"refresh-token"}}`),
			nativeRoute: false,
		},
		{
			id:          "wrong-type-entry-is-skipped-before-hosts",
			apps:        file(`{"github.com:Iv1.test":{"oauth_token":42}}`),
			hosts:       file(`{"github.com:Iv1.test":{"oauth_token":"hosts-token"}}`),
			nativeRoute: false,
		},
		{
			id:          "malformed-apps-json-falls-through-to-hosts",
			apps:        file(`{"github.com:Iv1.test":`),
			hosts:       file(`{"github.com:Iv1.test":{"oauth_token":"hosts-token"}}`),
			nativeRoute: false,
		},
		{
			id:          "empty-apps-token-falls-through-to-hosts",
			apps:        file(`{"github.com:Iv1.test":{"oauth_token":""}}`),
			hosts:       file(`{"github.com:Iv1.test":{"oauth_token":"hosts-token"}}`),
			nativeRoute: false,
		},
		{
			id:             "multiple-github-tokens-have-map-order-dependent-choice",
			apps:           file(`{"github.com:one":{"oauth_token":"one-token"},"github.com:two":{"oauth_token":"two-token"}}`),
			possibleTokens: []string{"one-token", "two-token"},
			nativeRoute:    false,
		},
		{
			id:             "multiple-fallback-tokens-have-map-order-dependent-choice",
			apps:           file(`{"enterprise-one":{"oauth_token":"one-token"},"enterprise-two":{"oauth_token":"two-token"}}`),
			possibleTokens: []string{"one-token", "two-token"},
			nativeRoute:    false,
		},
		{
			id:          "github-prefix-phase-precedes-other-hosts",
			apps:        file(`{"enterprise":{"oauth_token":"enterprise-token"},"github.com:one":{"oauth_token":"github-token"}}`),
			nativeRoute: false,
		},
		{
			id:            "single-reference-shaped-token-stays-on-go",
			apps:          file(`{"github.com:Iv1.test":{"oauth_token":"symvault://copilot/access-token"}}`),
			rustCandidate: true,
			nativeRoute:   false,
		},
		{
			id:          "missing-both-files-has-no-token",
			nativeRoute: true,
		},
	}
	fixture := copilotFileTokenOracleFixture{SchemaVersion: 1}
	for _, input := range inputs {
		if len(input.possibleTokens) != 0 {
			allowed := make(map[string]struct{}, len(input.possibleTokens))
			for _, token := range input.possibleTokens {
				allowed[token] = struct{}{}
			}
			for attempt := 0; attempt < 32; attempt++ {
				token, err := usage.BuildCopilotFileTokenOracle(input.apps, input.hosts)
				if err != nil {
					return copilotFileTokenOracleFixture{}, err
				}
				if _, ok := allowed[token]; !ok {
					return copilotFileTokenOracleFixture{}, fmt.Errorf("Copilot parser returned %q outside the normalized map-order set for %s", token, input.id)
				}
			}
			fixture.Cases = append(fixture.Cases, copilotFileTokenOracleCase{
				ID: input.id, AppsJSON: input.apps, HostsJSON: input.hosts,
				PossibleTokens: input.possibleTokens,
				SelectionRule:  "one of possible_tokens; Go map iteration order is unspecified",
				RustCandidate:  input.rustCandidate, NativeRoute: input.nativeRoute,
			})
			continue
		}
		token, err := usage.BuildCopilotFileTokenOracle(input.apps, input.hosts)
		if err != nil {
			return copilotFileTokenOracleFixture{}, err
		}
		fixture.Cases = append(fixture.Cases, copilotFileTokenOracleCase{
			ID: input.id, AppsJSON: input.apps, HostsJSON: input.hosts,
			Token: &token, RustCandidate: input.rustCandidate, NativeRoute: input.nativeRoute,
		})
	}
	return fixture, nil
}

func buildCopilotFileAuthenticatedReport(body []byte) (copilotFileReportFixture, error) {
	fixture := copilotFileReportFixture{}
	var err error
	fixture.Success, err = buildAuthenticatedProviderReportFrom("copilot", copilotFileOracleToken, body, http.StatusOK, true)
	if err != nil {
		return copilotFileReportFixture{}, err
	}
	for _, response := range []struct {
		status int
		body   []byte
	}{
		{http.StatusUnauthorized, []byte(`{"error":"nope"}`)},
		{http.StatusTooManyRequests, []byte(`{"error":"slow down"}`)},
		{http.StatusOK, []byte("{}")},
	} {
		report, reportErr := buildAuthenticatedProviderReportFrom("copilot", copilotFileOracleToken, response.body, response.status, true)
		if reportErr != nil {
			return copilotFileReportFixture{}, reportErr
		}
		fixture.Errors = append(fixture.Errors, struct {
			Status int          `json:"status"`
			Report usage.Report `json:"report"`
		}{Status: response.status, Report: report})
	}
	return fixture, nil
}

func buildClaudeFileTokenOracle() (claudeFileTokenOracleFixture, error) {
	inputs := []struct {
		id             string
		contents       string
		possibleTokens []string
	}{
		{
			id:       "default-account-precedes-other-accounts",
			contents: `{"oauthAccount":{"work":{"accessToken":"work-token"},"default":{"accessToken":"default-token"}}}`,
		},
		{
			id:       "single-nondefault-account-is-unambiguous",
			contents: `{"oauthAccount":{"spare":{"accessToken":"spare-token"}}}`,
		},
		{
			id:       "duplicate-account-key-uses-last-token",
			contents: `{"oauthAccount":{"default":{"accessToken":"first-token"},"default":{"accessToken":"last-token"}}}`,
		},
		{
			id:       "unknown-credential-metadata-is-ignored-by-go",
			contents: `{"version":1,"oauthAccount":{"default":{"accessToken":"default-token","refreshToken":"refresh-token"}}}`,
		},
		{
			id:       "struct-field-matching-is-case-insensitive",
			contents: `{"OAuthAccount":{"spare":{"AccessToken":"case-token"}}}`,
		},
		{
			id:       "duplicate-top-level-map-field-merges-entries",
			contents: `{"oauthAccount":{"default":{"accessToken":"default-token"}},"oauthAccount":{"work":{"accessToken":"work-token"}}}`,
		},
		{
			id:       "duplicate-struct-token-field-uses-last-value",
			contents: `{"oauthAccount":{"default":{"accessToken":"first-token","accessToken":"last-token"}}}`,
		},
		{
			id:       "wrong-typed-sibling-invalidates-whole-decode",
			contents: `{"oauthAccount":{"default":{"accessToken":"default-token"},"work":{"accessToken":42}}}`,
		},
		{
			id:       "malformed-json-yields-no-token",
			contents: `{"oauthAccount":{"default":{"accessToken":"partial-token"}}`,
		},
		{
			id:             "nondefault-multiple-token-accounts-are-map-order-dependent",
			contents:       `{"oauthAccount":{"work":{"accessToken":"work-token"},"spare":{"accessToken":"spare-token"}}}`,
			possibleTokens: []string{"spare-token", "work-token"},
		},
	}
	fixture := claudeFileTokenOracleFixture{SchemaVersion: 1}
	for _, input := range inputs {
		if len(input.possibleTokens) != 0 {
			allowed := make(map[string]struct{}, len(input.possibleTokens))
			for _, token := range input.possibleTokens {
				allowed[token] = struct{}{}
			}
			for attempt := 0; attempt < 16; attempt++ {
				token, err := usage.BuildClaudeFileTokenOracle([]byte(input.contents))
				if err != nil {
					return claudeFileTokenOracleFixture{}, err
				}
				if _, ok := allowed[token]; !ok {
					return claudeFileTokenOracleFixture{}, fmt.Errorf("Claude parser returned %q outside the normalized map-order set for %s", token, input.id)
				}
			}
			fixture.Cases = append(fixture.Cases, claudeFileTokenOracleCase{
				ID: input.id, Contents: input.contents, PossibleTokens: input.possibleTokens,
				SelectionRule: "one of possible_tokens; Go map iteration order is unspecified",
			})
			continue
		}
		token, err := usage.BuildClaudeFileTokenOracle([]byte(input.contents))
		if err != nil {
			return claudeFileTokenOracleFixture{}, err
		}
		fixture.Cases = append(fixture.Cases, claudeFileTokenOracleCase{
			ID: input.id, Contents: input.contents, Token: &token,
		})
	}
	return fixture, nil
}

func buildClaudeOAuthAuthenticatedReport(body []byte) (claudeOAuthReportFixture, error) {
	fixture := claudeOAuthReportFixture{}
	var err error
	fixture.Success, err = buildClaudeOAuthReport(200, body)
	if err != nil {
		return claudeOAuthReportFixture{}, err
	}
	fixture.FileSuccess, err = buildClaudeOAuthFileReport(200, body)
	if err != nil {
		return claudeOAuthReportFixture{}, err
	}
	for _, response := range []struct {
		status int
		body   []byte
	}{
		{http.StatusUnauthorized, []byte(`{"error":"nope"}`)},
		{http.StatusTooManyRequests, []byte(`{"error":"slow down"}`)},
		{http.StatusOK, []byte("not-json")},
	} {
		report, reportErr := buildClaudeOAuthReport(response.status, response.body)
		if reportErr != nil {
			return claudeOAuthReportFixture{}, reportErr
		}
		fixture.Errors = append(fixture.Errors, struct {
			Status int          `json:"status"`
			Report usage.Report `json:"report"`
		}{Status: response.status, Report: report})
		fileReport, fileErr := buildClaudeOAuthFileReport(response.status, response.body)
		if fileErr != nil {
			return claudeOAuthReportFixture{}, fileErr
		}
		fixture.FileErrors = append(fixture.FileErrors, struct {
			Status int          `json:"status"`
			Report usage.Report `json:"report"`
		}{Status: response.status, Report: fileReport})
	}
	return fixture, nil
}

func buildClaudeOAuthReport(status int, body []byte) (report usage.Report, retErr error) {
	return buildClaudeOAuthReportFrom(status, body, false)
}

func buildClaudeOAuthFileReport(status int, body []byte) (report usage.Report, retErr error) {
	return buildClaudeOAuthReportFrom(status, body, true)
}

func buildClaudeOAuthReportFrom(status int, body []byte, fileCredential bool) (report usage.Report, retErr error) {
	credentialEnv := []string{
		"HOME", "USERPROFILE", "ANTHROPIC_ADMIN_KEY", "ANTHROPIC_OAUTH_TOKEN",
		"CODEX_HOME", "HERMES_HOME", "KIMI_CODE_HOME", "CODEX_ACCESS_TOKEN",
		"COPILOT_ACCESS_TOKEN", "CURSOR_COOKIE", "KIMI_CODE_API_KEY", "KIMI_AUTH_TOKEN",
		"KIMI_CODE_BASE_URL", "MOONSHOT_API_KEY", "MOONSHOT_REGION",
		"NOUS_PORTAL_ACCESS_TOKEN", "HERMES_PORTAL_BASE_URL", "OPENCODE_COOKIE",
		"OPENCODE_WORKSPACE_ID", "OPENROUTER_API_KEY", "OPENROUTER_API_URL",
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
				return fmt.Errorf("restore %s after Claude OAuth oracle: %w", name, err)
			}
		}
		return nil
	}
	for _, name := range credentialEnv {
		if err := os.Unsetenv(name); err != nil {
			_ = restore()
			return usage.Report{}, fmt.Errorf("clear %s for Claude OAuth oracle: %w", name, err)
		}
	}
	defer func() {
		if err := restore(); err != nil && retErr == nil {
			retErr = err
		}
	}()
	home, err := os.MkdirTemp("", "symbrain-claude-oauth-oracle-home-")
	if err != nil {
		return usage.Report{}, fmt.Errorf("create isolated Claude OAuth oracle home: %w", err)
	}
	defer func() {
		if err := os.RemoveAll(home); err != nil && retErr == nil {
			retErr = fmt.Errorf("remove isolated Claude OAuth oracle home: %w", err)
		}
	}()
	if err := os.Setenv("HOME", home); err != nil {
		return usage.Report{}, fmt.Errorf("set isolated Claude OAuth oracle home: %w", err)
	}
	if err := os.Setenv("USERPROFILE", home); err != nil {
		return usage.Report{}, fmt.Errorf("set isolated Claude OAuth oracle profile: %w", err)
	}
	token := claudeOAuthOracleToken
	source := "env"
	if fileCredential {
		token = claudeOAuthFileOracleToken
		source = "file"
	}
	fileToken := "oracle-only-invalid-claude-oauth-file-shadowed-by-env"
	if fileCredential {
		fileToken = token
	}
	credentialsPath := filepath.Join(home, ".claude", ".credentials.json")
	credentials := fmt.Sprintf(`{"oauthAccount":{"work":{"accessToken":"%s"},"default":{"accessToken":"%s"}}}`,
		"oracle-only-invalid-claude-oauth-work", fileToken)
	if err := os.MkdirAll(filepath.Dir(credentialsPath), 0o700); err != nil {
		return usage.Report{}, fmt.Errorf("create isolated Claude credential directory: %w", err)
	}
	if err := os.WriteFile(credentialsPath, []byte(credentials), 0o600); err != nil {
		return usage.Report{}, fmt.Errorf("write synthetic Claude credential file: %w", err)
	}
	if !fileCredential {
		if err := os.Setenv("ANTHROPIC_OAUTH_TOKEN", token); err != nil {
			return usage.Report{}, fmt.Errorf("set synthetic Claude OAuth oracle token: %w", err)
		}
	}
	transport := &claudeOAuthOracleTransport{status: status, body: body}
	provider := usage.NewClaudeProvider(&http.Client{Transport: transport})
	if !provider.IsConfigured() || provider.AuthStatus().Source != source {
		return usage.Report{}, fmt.Errorf("Claude OAuth oracle source is %q, want %q", provider.AuthStatus().Source, source)
	}
	report = usage.BuildReport(context.Background(), []usage.Provider{provider})
	if len(report.Providers) != 1 || len(transport.requests) != 1 {
		return usage.Report{}, fmt.Errorf("Claude OAuth oracle produced %d providers and %d fixture requests, want one each", len(report.Providers), len(transport.requests))
	}
	request := transport.requests[0]
	if request.Method != http.MethodGet || request.URL.String() != "https://api.anthropic.com/api/oauth/usage" ||
		request.Header.Get("Authorization") != "Bearer "+token ||
		request.Header.Get("anthropic-beta") != "oauth-2025-04-20" {
		return usage.Report{}, fmt.Errorf("Claude OAuth oracle did not issue the expected authenticated usage request")
	}
	if snapshot := report.Providers[0].Snapshot; snapshot != nil {
		fixed := time.Date(2026, 8, 1, 0, 0, 0, 0, time.UTC)
		snapshot.FetchedAt = fixed
	}
	return report, nil
}

type claudeOAuthOracleTransport struct {
	status   int
	body     []byte
	requests []*http.Request
}

func (t *claudeOAuthOracleTransport) RoundTrip(request *http.Request) (*http.Response, error) {
	t.requests = append(t.requests, request.Clone(request.Context()))
	return &http.Response{
		StatusCode: t.status,
		Header:     http.Header{"Retry-After": []string{"17"}},
		Body:       io.NopCloser(bytes.NewReader(t.body)),
		Request:    request,
	}, nil
}

func buildCodexAuthenticatedReport(body []byte) (usage.Report, error) {
	return buildAuthenticatedProviderReport("codex", codexOracleToken, body)
}

func buildCodexFileAuthenticatedReport(body []byte) (codexFileReportFixture, error) {
	var fixture codexFileReportFixture
	var err error
	fixture.Success, err = buildAuthenticatedProviderReportFrom("codex", codexFileOracleToken, body, http.StatusOK, true)
	if err != nil {
		return codexFileReportFixture{}, err
	}
	for _, response := range []struct {
		status int
		body   []byte
	}{
		{http.StatusUnauthorized, []byte("{\"error\":\"nope\"}")},
		{http.StatusTooManyRequests, []byte("{\"error\":\"slow down\"}")},
		{http.StatusOK, []byte("{}")},
	} {
		report, reportErr := buildAuthenticatedProviderReportFrom("codex", codexFileOracleToken, response.body, response.status, true)
		if reportErr != nil {
			return codexFileReportFixture{}, reportErr
		}
		fixture.Errors = append(fixture.Errors, struct {
			Status int          `json:"status"`
			Report usage.Report `json:"report"`
		}{Status: response.status, Report: report})
	}
	return fixture, nil
}

func buildOpenRouterAuthenticatedReport(body []byte) (usage.Report, error) {
	return buildAuthenticatedProviderReport("openrouter", openRouterOracleToken, body)
}

func buildMoonshotAuthenticatedReport(body []byte) (usage.Report, error) {
	return buildAuthenticatedProviderReport("moonshot", moonshotOracleToken, body)
}

func buildCursorAuthenticatedReport(body []byte) (usage.Report, error) {
	return buildAuthenticatedProviderReport("cursor", cursorOracleToken, body)
}

func buildKimiAuthenticatedReport(body []byte) (usage.Report, error) {
	return buildAuthenticatedProviderReport("kimi", kimiOracleToken, body)
}

func buildNousAuthenticatedReport(body []byte) (usage.Report, error) {
	return buildAuthenticatedProviderReport("nous", nousOracleToken, body)
}

func buildNousFileAuthenticatedReport(body []byte) (nousFileReportFixture, error) {
	token := "header." + base64.RawURLEncoding.EncodeToString([]byte(`{"exp":4102444800}`)) + ".signature"
	fixture := nousFileReportFixture{}
	var err error
	fixture.Success, err = usage.BuildNousFileAuthenticatedReportOracle(token, body, http.StatusOK)
	if err != nil {
		return nousFileReportFixture{}, err
	}
	for _, response := range []struct {
		status int
		body   []byte
	}{
		{http.StatusUnauthorized, []byte(`{"error":"nope"}`)},
		{http.StatusTooManyRequests, []byte(`{"error":"slow down"}`)},
		{http.StatusOK, []byte(`{}`)},
	} {
		report, reportErr := usage.BuildNousFileAuthenticatedReportOracle(token, response.body, response.status)
		if reportErr != nil {
			return nousFileReportFixture{}, reportErr
		}
		fixture.Errors = append(fixture.Errors, struct {
			Status int          `json:"status"`
			Report usage.Report `json:"report"`
		}{Status: response.status, Report: report})
	}
	return fixture, nil
}

func buildOpenCodeAuthenticatedReport(workspaceBody, body []byte) (usage.Report, error) {
	return buildAuthenticatedProviderReport("opencode", openCodeOracleToken, body, workspaceBody)
}

func buildAuthenticatedProviderReport(provider, token string, body []byte, extra ...[]byte) (usage.Report, error) {
	return buildAuthenticatedProviderReportFrom(provider, token, body, http.StatusOK, false, extra...)
}

func buildAuthenticatedProviderReportFrom(provider, token string, body []byte, status int, fileCredential bool, extra ...[]byte) (usage.Report, error) {
	// Keep the full ten-provider production graph deterministic without
	// inheriting a developer's credential environment or home directory.
	credentialEnv := []string{
		"HOME", "USERPROFILE", "CODEX_HOME", "HERMES_HOME", "KIMI_CODE_HOME",
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
	if err := os.Setenv("USERPROFILE", home); err != nil {
		_ = os.RemoveAll(home)
		_ = restore()
		return usage.Report{}, fmt.Errorf("set isolated usage oracle user profile: %w", err)
	}
	envName := map[string]string{
		"claude-admin": "ANTHROPIC_ADMIN_KEY",
		"copilot":      "COPILOT_ACCESS_TOKEN",
		"codex":        "CODEX_ACCESS_TOKEN",
		"cursor":       "CURSOR_COOKIE",
		"kimi":         "KIMI_CODE_API_KEY",
		"moonshot":     "MOONSHOT_API_KEY",
		"nous":         "NOUS_PORTAL_ACCESS_TOKEN",
		"opencode":     "OPENCODE_COOKIE",
		"openrouter":   "OPENROUTER_API_KEY",
	}[provider]
	if envName == "" {
		_ = os.RemoveAll(home)
		_ = restore()
		return usage.Report{}, fmt.Errorf("unsupported authenticated usage oracle provider %q", provider)
	}
	if fileCredential {
		if provider == "copilot" {
			path := filepath.Join(home, ".config", "github-copilot", "apps.json")
			if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
				_ = os.RemoveAll(home)
				_ = restore()
				return usage.Report{}, fmt.Errorf("create isolated Copilot credential directory: %w", err)
			}
			contents := fmt.Sprintf("{\"github.com:Iv1.oracle\":{\"oauth_token\":%q}}", token)
			if err := os.WriteFile(path, []byte(contents), 0o600); err != nil {
				_ = os.RemoveAll(home)
				_ = restore()
				return usage.Report{}, fmt.Errorf("write synthetic Copilot apps file: %w", err)
			}
		} else if provider == "codex" {
			path := filepath.Join(home, ".codex", "auth.json")
			if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
				_ = os.RemoveAll(home)
				_ = restore()
				return usage.Report{}, fmt.Errorf("create isolated Codex credential directory: %w", err)
			}
			contents := fmt.Sprintf("{\"access_token\":%q}", token)
			if err := os.WriteFile(path, []byte(contents), 0o600); err != nil {
				_ = os.RemoveAll(home)
				_ = restore()
				return usage.Report{}, fmt.Errorf("write synthetic Codex auth file: %w", err)
			}
		} else {
			_ = os.RemoveAll(home)
			_ = restore()
			return usage.Report{}, fmt.Errorf("file credential oracle is unsupported for %q", provider)
		}
	} else if err := os.Setenv(envName, token); err != nil {
		_ = os.RemoveAll(home)
		_ = restore()
		return usage.Report{}, fmt.Errorf("set synthetic %s fixture env: %w", provider, err)
	}
	var report usage.Report
	var buildErr error
	switch provider {
	case "claude-admin":
		report, buildErr = usage.BuildClaudeAdminAuthenticatedReportOracle(body)
	case "copilot":
		if fileCredential {
			report, buildErr = usage.BuildCopilotFileAuthenticatedReportOracle(body, status)
		} else {
			report, buildErr = usage.BuildCopilotAuthenticatedReportOracle(body)
		}
	case "codex":
		if fileCredential {
			report, buildErr = usage.BuildCodexFileAuthenticatedReportOracle(body, status)
		} else {
			report, buildErr = usage.BuildCodexAuthenticatedReportOracle(body)
		}
	case "openrouter":
		report, buildErr = usage.BuildOpenRouterAuthenticatedReportOracle(body)
	case "moonshot":
		report, buildErr = usage.BuildMoonshotAuthenticatedReportOracle(body)
	case "cursor":
		report, buildErr = usage.BuildCursorAuthenticatedReportOracle(body)
	case "kimi":
		report, buildErr = usage.BuildKimiAuthenticatedReportOracle(body)
	case "nous":
		report, buildErr = usage.BuildNousAuthenticatedReportOracle(body)
	case "opencode":
		if len(extra) != 1 {
			buildErr = fmt.Errorf("OpenCode report oracle requires a workspace fixture")
		} else {
			report, buildErr = usage.BuildOpenCodeAuthenticatedReportOracle(extra[0], body)
		}
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
