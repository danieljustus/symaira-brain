package main

import (
	"bytes"
	"crypto/sha256"
	"fmt"
	"os"
	"os/exec"
	"runtime"
)

const oracleRevision = "a92385d2deecc08d1fd96869908b81b7abd355fe"
const oracleToolchain = "go1.26.7"

var sourcePaths = []string{
	"internal/usage/opencode.go",
	"internal/usage/opencode_parse.go",
	"internal/usage/provider.go",
	"internal/usage/schema.go",
	"internal/usage/http_security.go",
	"internal/usage/creds.go",
	"internal/usage/format.go",
	"internal/usage/testdata/opencode-workspaces.txt",
	"internal/usage/testdata/opencode-subscription-json.txt",
	"go.mod", "go.sum",
}

var harnessPaths = []string{
	"scripts/usage-request-oracle/opencode/main.go",
	"scripts/usage-request-oracle/opencode/cases.go",
	"scripts/usage-request-oracle/opencode/review_cases.go",
	"scripts/usage-request-oracle/opencode/provenance.go",
	"rust/symbrain-usage/src/opencode_discovery_tests.rs",
	"rust/symbrain-usage/src/opencode_provenance_tests.rs",
}

func digestText(data []byte) string {
	// Git text checkouts may use CRLF. Protocol payloads are not normalized.
	return fmt.Sprintf("%x", sha256.Sum256(bytes.ReplaceAll(data, []byte("\r\n"), []byte("\n"))))
}

func recordProvenance(p *provenance) error {
	if runtime.Version() != oracleToolchain {
		return fmt.Errorf("oracle needs %s, got %s", oracleToolchain, runtime.Version())
	}
	// The release revision survives squash integration of this new harness.
	if err := exec.Command("git", "merge-base", "--is-ancestor", oracleRevision, "HEAD").Run(); err != nil {
		return fmt.Errorf("oracle revision is not an ancestor: %w", err)
	}
	p.OracleRevision, p.Toolchain = oracleRevision, oracleToolchain
	p.SourceHashes = make(map[string]string, len(sourcePaths))
	p.HarnessHashes = make(map[string]string, len(harnessPaths))
	for _, path := range sourcePaths {
		pinned, err := exec.Command("git", "show", oracleRevision+":"+path).Output()
		if err != nil {
			return fmt.Errorf("read pinned source %s: %w", path, err)
		}
		current, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		if digestText(pinned) != digestText(current) {
			return fmt.Errorf("oracle source changed: %s", path)
		}
		p.SourceHashes[path] = digestText(pinned)
	}
	for _, path := range harnessPaths {
		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		p.HarnessHashes[path] = digestText(data)
	}
	return nil
}
