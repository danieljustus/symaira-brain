package main

import (
	"bytes"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"runtime"
	"strings"

	"github.com/danieljustus/symaira-brain/guard/cmd/symguard/doctor"
)

type doctorCase struct {
	ID         string `json:"id"`
	Setup      string `json:"setup"`
	ExitCode   int    `json:"exit_code"`
	OutputJSON string `json:"output_json"`
}

type doctorSuite struct {
	Cases  []doctorCase `json:"cases"`
	Source string       `json:"source"`
}

func writeFile(path, content string) {
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		panic(err)
	}
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		panic(err)
	}
}

func normalize(root, output string) string {
	// The command prints absolute fixture paths, the building toolchain, and
	// the runtime OS/Arch. All three differ per invocation (TMPDIR), per Go
	// toolchain, and per runner platform, so a frozen fixture can only be
	// guarded once they are placeholders. Recorded once on a macOS machine,
	// this fixture baked "OS/Arch: darwin/arm64" in as literal text until a
	// Linux CI runner (the first time this check ran on any runner at all —
	// see #631) surfaced the gap: "OS/Arch:" was never normalized, only
	// "Go:" was. Both toolchain and platform lines are explicit accepted
	// differences: a Rust binary reports its own toolchain and its own real
	// OS/Arch, never a value borrowed from this Go fixture.
	output = strings.ReplaceAll(output, root, "<root>")
	output = goVersionLine.ReplaceAllString(output, "${1}<go>")
	return osArchLine.ReplaceAllString(output, "${1}<os/arch>")
}

var (
	goVersionLine = regexp.MustCompile(`(Go:\s+)\S+`)
	osArchLine    = regexp.MustCompile(`(OS/Arch:\s+)\S+`)
)

func runCase(root, id string, setup func(string) error) (string, int, error) {
	caseRoot := filepath.Join(root, id)
	if err := os.RemoveAll(caseRoot); err != nil {
		return "", 0, fmt.Errorf("remove %s: %w", caseRoot, err)
	}
	if err := os.MkdirAll(filepath.Join(caseRoot, "home", ".config", "symguard"), 0o755); err != nil {
		return "", 0, fmt.Errorf("mkdir config: %w", err)
	}
	if err := os.MkdirAll(filepath.Join(caseRoot, "data", "symguard"), 0o755); err != nil {
		return "", 0, fmt.Errorf("mkdir data: %w", err)
	}
	home := filepath.Join(caseRoot, "home")
	for _, relative := range []string{
		".config/hermes",
		".cursor",
		".vscode",
		".config/opencode",
		".config/claude",
	} {
		if err := os.MkdirAll(filepath.Join(home, relative), 0o755); err != nil {
			return "", 0, fmt.Errorf("mkdir discovery source: %w", err)
		}
	}
	if err := setup(caseRoot); err != nil {
		return "", 0, fmt.Errorf("setup %s: %w", id, err)
	}

	configHome := filepath.Join(home, ".config")
	dataHome := filepath.Join(caseRoot, "data")
	symguardConfig := filepath.Join(configHome, "symguard", "config.toml")
	os.Setenv("HOME", home)
	os.Setenv("USERPROFILE", home)
	os.Setenv("XDG_CONFIG_HOME", configHome)
	os.Setenv("XDG_DATA_HOME", dataHome)
	os.Setenv("SYMGUARD_CONFIG", symguardConfig)

	var buf bytes.Buffer
	exit := doctor.Run(&buf)
	return normalize(root, buf.String()), exit, nil
}

func buildSuite() doctorSuite {
	root, err := os.MkdirTemp("", "guard-doctor-oracle-")
	if err != nil {
		panic(err)
	}
	defer os.RemoveAll(root)

	var cases []doctorCase
	add := func(id, setup string, fn func(string) error) {
		output, exit, err := runCase(root, id, fn)
		if err != nil {
			panic(err)
		}
		cases = append(cases, doctorCase{
			ID:         id,
			Setup:      setup,
			ExitCode:   exit,
			OutputJSON: string(mustJSON(output)),
		})
	}

	add("empty_machine", "no config file, no audit log, no discovered servers", func(root string) error { return nil })

	add("healthy_config", "valid config with 1 rule, 1 allowlist entry", func(root string) error {
		config := fmt.Sprintf(`[defaults]
shell = "allow"
read_secret = "deny"

[[rules]]
match.server = "symmemory"
match.tool = "memory_search"
decision = "allow"

[spawn]
[[spawn.allowlist]]
path = %q
`, oracleCommand())
		return os.WriteFile(filepath.Join(root, "home", ".config", "symguard", "config.toml"), []byte(config), 0o644)
	})

	add("config_error", "config file exists but contains invalid TOML", func(root string) error {
		return os.WriteFile(filepath.Join(root, "home", ".config", "symguard", "config.toml"), []byte("not [valid = toml"), 0o644)
	})

	add("config_error_other_ascii", "missing equals with a printable ASCII offender", func(root string) error {
		return os.WriteFile(filepath.Join(root, "home", ".config", "symguard", "config.toml"), []byte("name ! value"), 0o644)
	})

	add("config_error_later_line", "missing equals on a later line", func(root string) error {
		return os.WriteFile(filepath.Join(root, "home", ".config", "symguard", "config.toml"), []byte("valid = \"ok\"\nname [value"), 0o644)
	})

	add("audit_log_without_anchor", "audit log exists but no anchor file (expected pre-Phase-3 state)", func(root string) error {
		return os.WriteFile(filepath.Join(root, "data", "symguard", "audit.log"), []byte("{\"entry_id\":\"1\"}\n"), 0o644)
	})

	add("audit_log_corrupt_anchor", "audit log exists with corrupt anchor file", func(root string) error {
		if err := os.WriteFile(filepath.Join(root, "data", "symguard", "audit.log"), []byte("{\"entry_id\":\"1\"}\n"), 0o644); err != nil {
			return err
		}
		return os.WriteFile(filepath.Join(root, "data", "symguard", "audit.log.anchor"), []byte("not json"), 0o644)
	})

	add("discovered_server_denied", "Cursor mcp.json with server not on allowlist", func(root string) error {
		cursor := filepath.Join(root, "home", ".cursor", "mcp.json")
		if err := os.MkdirAll(filepath.Dir(cursor), 0o755); err != nil {
			return err
		}
		return os.WriteFile(cursor, []byte(`{"mcpServers":{"demo":{"command":"/usr/bin/true","args":["--once"]}}}`), 0o644)
	})

	add("discovered_server_secret_risk", "Cursor mcp.json with server carrying a plaintext secret env key", func(root string) error {
		cursor := filepath.Join(root, "home", ".cursor", "mcp.json")
		if err := os.MkdirAll(filepath.Dir(cursor), 0o755); err != nil {
			return err
		}
		return os.WriteFile(cursor, []byte(`{"mcpServers":{"server-with-secret":{"command":"/usr/bin/env","env":{"SECRET_KEY":"literal"}}}}`), 0o644)
	})

	return doctorSuite{
		Cases:  cases,
		Source: "guard/cmd/symguard/doctor/command.go + checks.go",
	}
}

func oracleCommand() string {
	if runtime.GOOS != "windows" {
		return "/usr/bin/true"
	}
	systemRoot := os.Getenv("SystemRoot")
	if systemRoot == "" {
		systemRoot = os.Getenv("windir")
	}
	if systemRoot == "" {
		systemRoot = `C:\Windows`
	}
	return filepath.Join(systemRoot, "System32", "where.exe")
}

func mustJSON(value any) []byte {
	data, err := json.Marshal(value)
	if err != nil {
		panic(err)
	}
	return data
}

func main() {
	check := flag.Bool("check", false, "fail if generated output does not match existing file")
	defaultOutput := os.Getenv("SYMBRAIN_GUARD_DOCTOR_ORACLE_FIXTURE")
	if defaultOutput == "" {
		defaultOutput = "rust/symbrain-guard-core/tests/fixtures/doctor_oracle.json"
	}
	output := flag.String("output", defaultOutput, "output expectations path")
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
			fmt.Fprintf(os.Stderr, "%s is out of date; run go run ./guard/scripts/guard-doctor-oracle\n", *output)
			os.Exit(1)
		}
		fmt.Printf("PASS: guard doctor oracle deterministic check passed (0 drift on %d cases)\n", len(suite.Cases))
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
