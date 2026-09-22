// Command secret-oracle freezes internal/memory/secrets' resolution behavior
// by driving the real secrets package against a fake `symvault` executable
// injected on PATH (never the real binary, keychain, or network).
//
// Frozen seams: scheme acceptance/rejection, deprecated vault:// alias
// equivalence, env-fallback precedence via ResolveOrEnv, exact error bytes for
// missing paths/invalid paths/absent binaries, and the subprocess timeout
// config surface (corekit secretref.DefaultTimeout).
//
// Output: rust/symbrain-usage/tests/fixtures/secret_oracle.json, consumed by
// rust/symbrain-usage/tests/secret_oracle_tests.rs.
//
//	go run ./scripts/secret-oracle            # (re)write the fixture
//	go run ./scripts/secret-oracle -check     # fail on any drift
package main

import (
	"bytes"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/secrets"
	"github.com/danieljustus/symaira-corekit/secretref"
)

const outputDefault = "rust/symbrain-usage/tests/fixtures/secret_oracle.json"

// oracleEnvNames are cleared before every case so ambient developer
// environment can never leak into the frozen bytes.
var oracleEnvNames = []string{
	"SECRET_ORACLE_FALLBACK",
	"SECRET_ORACLE_PRESENT",
	"SECRET_ORACLE_ABSENT",
	"SECRET_ORACLE_OR_ENV",
}

type symvaultSpec struct {
	Mode    string `json:"mode"` // "fake" runs, "absent" is not on PATH
	Stdout  string `json:"stdout,omitempty"`
	Stderr  string `json:"stderr,omitempty"`
	Exit    int    `json:"exit,omitempty"`
	SleepMS int    `json:"sleep_ms,omitempty"`
}

type inputSpec struct {
	Value       string            `json:"value"`
	EnvFallback string            `json:"env_fallback,omitempty"`
	EnvName     string            `json:"env_name,omitempty"`
	Env         map[string]string `json:"env,omitempty"`
	Symvault    *symvaultSpec     `json:"symvault,omitempty"`
}

type oracleCase struct {
	ID                string    `json:"id"`
	Kind              string    `json:"kind"` // scheme | resolve | resolve_or_env | timeout
	Input             inputSpec `json:"input"`
	Timeout           string    `json:"timeout,omitempty"`
	Success           *bool     `json:"success,omitempty"`
	Value             string    `json:"value,omitempty"`
	Error             string    `json:"error,omitempty"`
	Argv              []string  `json:"argv"`
	IsVaultURI        *bool     `json:"is_vault_uri,omitempty"`
	IsSecretReference *bool     `json:"is_secret_reference,omitempty"`
}

type suite struct {
	SchemaVersion  int          `json:"schema_version"`
	DefaultTimeout string       `json:"default_timeout"`
	Cases          []oracleCase `json:"cases"`
}

// fakeSymvaultSource is the compiled stand-in for the fake `symvault`
// executable: it appends its argv (one argument per line) to the path declared
// in the JSON sidecar next to itself, then obeys the canned stdout/stderr/
// exit/sleep. It is a native binary rather than a script so no shell
// interpreter is involved anywhere in the oracle (internal/security rejects
// shell literals in walked Go files) and resolution still works with PATH
// pointing at the fake bin directory alone.
const fakeSymvaultSource = `package main

import (
	"encoding/json"
	"os"
	"path/filepath"
	"time"
)

func main() {
	// exec.Command sets argv[0] to the bare name passed by the caller, so
	// resolve the real running binary path first and fall back to argv[0].
	self, err := os.Executable()
	if err != nil {
		self = os.Args[0]
		if !filepath.IsAbs(self) {
			if abs, absErr := filepath.Abs(self); absErr == nil {
				self = abs
			}
		}
	}
	data, err := os.ReadFile(filepath.Join(filepath.Dir(self), "symvault.spec"))
	if err != nil {
		os.Exit(127)
	}
	var m map[string]any
	if err := json.Unmarshal(data, &m); err != nil {
		os.Exit(127)
	}
	argsPath, _ := m["args_path"].(string)
	stdout, _ := m["stdout"].(string)
	stderr, _ := m["stderr"].(string)
	exit, _ := m["exit"].(float64)
	sleepMS, _ := m["sleep_ms"].(float64)
	// One line per argument; with no operands POSIX printf '%s\n' still runs
	// once, producing a lone newline.
	buf := []byte{}
	if len(os.Args) == 1 {
		buf = append(buf, '\n')
	}
	for _, a := range os.Args[1:] {
		buf = append(buf, a...)
		buf = append(buf, '\n')
	}
	if f, err := os.OpenFile(argsPath, os.O_WRONLY|os.O_CREATE|os.O_APPEND, 0o644); err == nil {
		_, _ = f.Write(buf)
		_ = f.Close()
	}
	if sleepMS > 0 {
		time.Sleep(time.Duration(sleepMS) * time.Millisecond)
		os.Exit(0)
	}
	_, _ = os.Stdout.WriteString(stdout)
	_, _ = os.Stderr.WriteString(stderr)
	os.Exit(int(exit))
}
`

var (
	fakeBuildOnce sync.Once
	fakeBuildDir  string
	fakeBinPath   string
	fakeBuildErr  error
)

// fakeSymvaultBin compiles fakeSymvaultSource once per oracle run.
func fakeSymvaultBin() (string, error) {
	fakeBuildOnce.Do(func() {
		dir, err := os.MkdirTemp("", "secret-oracle-fake-")
		if err != nil {
			fakeBuildErr = err
			return
		}
		fakeBuildDir = dir
		src := filepath.Join(dir, "main.go")
		if err := os.WriteFile(src, []byte(fakeSymvaultSource), 0o644); err != nil {
			fakeBuildErr = err
			return
		}
		out := filepath.Join(dir, "symvault")
		cmd := exec.Command("go", "build", "-o", out, src)
		if output, err := cmd.CombinedOutput(); err != nil {
			fakeBuildErr = fmt.Errorf("build fake symvault: %w: %s", err, output)
			return
		}
		fakeBinPath = out
	})
	return fakeBinPath, fakeBuildErr
}

// cleanupFakeBuild removes the once-built helper binary.
func cleanupFakeBuild() {
	if fakeBuildDir != "" {
		_ = os.RemoveAll(fakeBuildDir)
	}
}

// writeFakeSymvault places the compiled fake at binDir/symvault and writes the
// case spec as a JSON sidecar next to it (argv logging target, canned
// stdout/stderr, exit code, sleep).
func writeFakeSymvault(binDir, argsPath string, spec symvaultSpec) {
	bin, err := fakeSymvaultBin()
	if err != nil {
		panic(err)
	}
	payload, err := os.ReadFile(bin)
	if err != nil {
		panic(err)
	}
	type sidecarSpec struct {
		ArgsPath string `json:"args_path"`
		Stdout   string `json:"stdout"`
		Stderr   string `json:"stderr"`
		Exit     int    `json:"exit"`
		SleepMS  int    `json:"sleep_ms"`
	}
	data, err := json.Marshal(sidecarSpec{argsPath, spec.Stdout, spec.Stderr, spec.Exit, spec.SleepMS})
	if err != nil {
		panic(err)
	}
	if err := os.WriteFile(filepath.Join(binDir, "symvault"), payload, 0o755); err != nil {
		panic(err)
	}
	if err := os.WriteFile(filepath.Join(binDir, "symvault.spec"), data, 0o644); err != nil {
		panic(err)
	}
}

type savedEnv struct {
	key string
	val string
	ok  bool
}

type caseSetup struct {
	argsPath string
	restore  func()
}

// setupCase gives the case a throwaway PATH (fake bin dir or an empty dir so
// symvault is unresolvable), clears every oracle env var, applies the case
// env, and returns a restore func.
func setupCase(env map[string]string, spec *symvaultSpec) caseSetup {
	dir, err := os.MkdirTemp("", "secret-oracle-case")
	if err != nil {
		panic(err)
	}
	binDir := filepath.Join(dir, "bin")
	if err := os.MkdirAll(binDir, 0o755); err != nil {
		panic(err)
	}
	argsPath := filepath.Join(dir, "argv.txt")
	if spec != nil && spec.Mode == "fake" {
		writeFakeSymvault(binDir, argsPath, *spec)
	}
	oldPath := os.Getenv("PATH")
	if err := os.Setenv("PATH", binDir); err != nil {
		panic(err)
	}
	var saved []savedEnv
	capture := func(key string) {
		val, ok := os.LookupEnv(key)
		saved = append(saved, savedEnv{key: key, val: val, ok: ok})
	}
	for _, name := range oracleEnvNames {
		capture(name)
		if err := os.Unsetenv(name); err != nil {
			panic(err)
		}
	}
	for key, value := range env {
		capture(key)
		if err := os.Setenv(key, value); err != nil {
			panic(err)
		}
	}
	return caseSetup{argsPath: argsPath, restore: func() {
		if err := os.Setenv("PATH", oldPath); err != nil {
			panic(err)
		}
		for i := len(saved) - 1; i >= 0; i-- { // LIFO: a var set after its capture wins
			entry := saved[i]
			if entry.ok {
				if err := os.Setenv(entry.key, entry.val); err != nil {
					panic(err)
				}
			} else if err := os.Unsetenv(entry.key); err != nil {
				panic(err)
			}
		}
		if err := os.RemoveAll(dir); err != nil {
			panic(err)
		}
	}}
}

func readArgv(path string) []string {
	data, err := os.ReadFile(path)
	if err != nil || len(data) == 0 {
		return nil
	}
	return strings.Split(strings.TrimSuffix(string(data), "\n"), "\n")
}

func encodeResult(got string, err error) (bool, string, string) {
	if err != nil {
		return false, "", err.Error()
	}
	return true, got, ""
}

func schemeCase(id, value string) oracleCase {
	vaultURI := secrets.IsVaultURI(value)
	reference := secrets.IsSecretReference(value)
	return oracleCase{
		ID:                id,
		Kind:              "scheme",
		Input:             inputSpec{Value: value},
		Argv:              nil,
		IsVaultURI:        &vaultURI,
		IsSecretReference: &reference,
	}
}

// resolveCase runs the real secrets.Resolve under the case setup. timeout>0
// shrinks corekit's exported secretref.DefaultTimeout for the call, mirroring
// how Go tests bound the subprocess without sleeping for the real 5s.
func resolveCase(id string, in inputSpec, timeout time.Duration) oracleCase {
	setup := setupCase(in.Env, in.Symvault)
	defer setup.restore()
	if timeout > 0 {
		previous := secretref.DefaultTimeout
		secretref.DefaultTimeout = timeout
		defer func() { secretref.DefaultTimeout = previous }()
	}
	got, err := secrets.Resolve(in.Value, in.EnvFallback)
	argv := readArgv(setup.argsPath)
	success, value, message := encodeResult(got, err)
	result := oracleCase{
		ID:      id,
		Kind:    "resolve",
		Input:   in,
		Success: &success,
		Value:   value,
		Error:   message,
		Argv:    argv,
	}
	if timeout > 0 {
		result.Kind = "timeout"
		result.Timeout = timeout.String()
	}
	return result
}

func resolveOrEnvCase(id string, in inputSpec) oracleCase {
	setup := setupCase(in.Env, in.Symvault)
	defer setup.restore()
	got, err := secrets.ResolveOrEnv(in.Value, in.EnvName)
	argv := readArgv(setup.argsPath)
	success, value, message := encodeResult(got, err)
	return oracleCase{
		ID:      id,
		Kind:    "resolve_or_env",
		Input:   in,
		Success: &success,
		Value:   value,
		Error:   message,
		Argv:    argv,
	}
}

func buildCases() []oracleCase {
	const fallbackName = "SECRET_ORACLE_FALLBACK"
	failing := &symvaultSpec{Mode: "fake", Stderr: "unknown path\n", Exit: 1}
	working := &symvaultSpec{Mode: "fake", Stdout: "  resolved-jwt-secret \n\n"}
	absent := &symvaultSpec{Mode: "absent"}
	cases := []oracleCase{
		// Scheme acceptance and rejection (pure classification, no subprocess).
		schemeCase("canonical_symvault_scheme", "symvault://service/path"),
		schemeCase("deprecated_vault_scheme", "vault://service/path"),
		schemeCase("env_scheme", "env://API_TOKEN"),
		schemeCase("keychain_scheme", "keychain://service/account"),
		schemeCase("plain_value", "literal-secret"),
		schemeCase("empty_value", ""),
		schemeCase("near_miss_env_scheme", "env:API_TOKEN"),
		schemeCase("case_mismatch_scheme", "SymVaulT://case-sensitive"),
		schemeCase("absolute_path", "/path/to/file"),

		// Resolution through the real subprocess path.
		resolveCase("canonical_ok", inputSpec{Value: "symvault://symaira/memory/jwt", Symvault: working}, 0),
		resolveCase("alias_ok_equivalent", inputSpec{Value: "vault://symaira/memory/jwt", Symvault: working}, 0),
		resolveCase("alias_missing_path_error_bytes", inputSpec{Value: "vault://unknown/path", Symvault: failing}, 0),
		resolveCase("missing_path_error_names_env_fallback", inputSpec{
			Value: "symvault://unknown/path", EnvFallback: fallbackName, Symvault: failing,
		}, 0),
		resolveCase("env_fallback_wins_over_symvault_error", inputSpec{
			Value: "vault://unknown/path", EnvFallback: fallbackName,
			Env: map[string]string{fallbackName: "fallback-secret-value"}, Symvault: failing,
		}, 0),
		resolveCase("env_reference_missing_ignores_fallback", inputSpec{
			Value: "env://SECRET_ORACLE_ABSENT", EnvFallback: fallbackName,
			Env: map[string]string{fallbackName: "fallback-secret-value"}, Symvault: absent,
		}, 0),
		resolveCase("env_reference_resolves", inputSpec{
			Value: "env://SECRET_ORACLE_PRESENT",
			Env:   map[string]string{"SECRET_ORACLE_PRESENT": "env-reference-value"}, Symvault: absent,
		}, 0),
		resolveCase("symvault_absent_on_path", inputSpec{
			Value: "symvault://symaira/memory/jwt", Symvault: absent,
		}, 0),
		resolveCase("empty_secret_from_symvault", inputSpec{
			Value: "symvault://empty/path", Symvault: &symvaultSpec{Mode: "fake"},
		}, 0),
		resolveCase("invalid_path_empty", inputSpec{Value: "symvault://", Symvault: absent}, 0),
		resolveCase("invalid_path_leading_dash", inputSpec{Value: "symvault://-flag/path", Symvault: absent}, 0),
		resolveCase("invalid_path_control_character", inputSpec{Value: "symvault://bad\x01path", Symvault: absent}, 0),
		resolveCase("invalid_path_null_byte", inputSpec{Value: "symvault://bad\x00path", Symvault: absent}, 0),

		// ResolveOrEnv precedence.
		resolveOrEnvCase("or_env_value_wins", inputSpec{
			Value: "plain-secret", EnvName: fallbackName,
			Env: map[string]string{fallbackName: "or-env-value"}, Symvault: absent,
		}),
		resolveOrEnvCase("or_env_env_used_when_value_empty", inputSpec{
			Value: "", EnvName: "SECRET_ORACLE_OR_ENV",
			Env: map[string]string{"SECRET_ORACLE_OR_ENV": "or-env-value"}, Symvault: absent,
		}),
		resolveOrEnvCase("or_env_both_empty_returns_empty", inputSpec{
			Value: "", EnvName: "SECRET_ORACLE_OR_ENV", Symvault: absent,
		}),
		resolveOrEnvCase("or_env_vault_reference_uses_env_as_fallback", inputSpec{
			Value: "vault://unknown/path", EnvName: "SECRET_ORACLE_OR_ENV",
			Env: map[string]string{"SECRET_ORACLE_OR_ENV": "or-env-fallback-value"}, Symvault: absent,
		}),

		// Timeout config surface: DefaultTimeout shrinks and the exact
		// deadline bytes surface through the wrapped error. The deadline
		// keeps wide margin over helper spawn latency so the fake's argv
		// log always lands before the kill — a tighter deadline races it.
		resolveCase("shrunk_timeout_error_bytes", inputSpec{
			Value: "symvault://slow/path", Symvault: &symvaultSpec{Mode: "fake", SleepMS: 3000},
		}, 1000*time.Millisecond),
	}
	return cases
}

func setupIsolatedHome() func() {
	dir, err := os.MkdirTemp("", "secret-oracle-home")
	if err != nil {
		panic(err)
	}
	variables := []string{"HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME"}
	saved := make([]savedEnv, 0, len(variables))
	for _, name := range variables {
		val, ok := os.LookupEnv(name)
		saved = append(saved, savedEnv{key: name, val: val, ok: ok})
		if err := os.Setenv(name, filepath.Join(dir, strings.ToLower(name))); err != nil {
			panic(err)
		}
	}
	return func() {
		for _, entry := range saved {
			if entry.ok {
				if err := os.Setenv(entry.key, entry.val); err != nil {
					panic(err)
				}
			} else if err := os.Unsetenv(entry.key); err != nil {
				panic(err)
			}
		}
		if err := os.RemoveAll(dir); err != nil {
			panic(err)
		}
	}
}

func generate() suite {
	// Recorded before any case may shrink it: the shipped default surface.
	defaultTimeout := secretref.DefaultTimeout.String()
	return suite{
		SchemaVersion:  1,
		DefaultTimeout: defaultTimeout,
		Cases:          buildCases(),
	}
}

func main() {
	check := flag.Bool("check", false, "fail if generated output does not match existing file")
	output := flag.String("output", outputDefault, "output expectations path")
	flag.Parse()

	restoreHome := setupIsolatedHome()
	defer restoreHome()
	defer cleanupFakeBuild()

	suite := generate()
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
			fmt.Fprintf(os.Stderr, "%s is out of date; run go run ./scripts/secret-oracle\n", *output)
			os.Exit(1)
		}
		fmt.Printf("PASS: secret oracle deterministic check passed (%d cases)\n", len(suite.Cases))
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
