package capability

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"testing"
	"testing/synctest"
	"time"

	"github.com/danieljustus/symaira-brain/guard/internal/policy"
)

const capabilityOracleRevision = "0ccb0fe6c5afe668714d647eaa3497f8e6ed3f79"

type capabilityCase struct {
	ID        string         `json:"id"`
	Kind      string         `json:"kind"`
	MasterHex string         `json:"master_hex,omitempty"`
	Token     string         `json:"token,omitempty"`
	Claims    *Claims        `json:"claims,omitempty"`
	Scope     []string       `json:"scope"`
	Target    string         `json:"target,omitempty"`
	Result    *policy.Result `json:"result,omitempty"`
	Now       int64          `json:"now"`
	Output    string         `json:"output,omitempty"`
	Error     string         `json:"error,omitempty"`
	ErrorKind string         `json:"error_kind,omitempty"`
}

type capabilitySuite struct {
	OracleRevision  string            `json:"oracle_revision"`
	SourceSHA256    map[string]string `json:"source_sha256"`
	GeneratorSHA256 map[string]string `json:"generator_sha256"`
	Cases           []capabilityCase  `json:"cases"`
}

func oracleJSON(t *testing.T, value any) string {
	t.Helper()
	b, err := json.Marshal(value)
	if err != nil {
		t.Fatal(err)
	}
	return string(b)
}

func oracleError(err error) (string, string) {
	if err == nil {
		return "", ""
	}
	for _, candidate := range []struct {
		err  error
		kind string
	}{
		{ErrNoKeyMaterial, "no_key_material"}, {ErrMalformed, "malformed"},
		{ErrInvalidSignature, "invalid_signature"}, {ErrExpired, "expired"},
		{ErrInvalidClaims, "invalid_claims"},
	} {
		if errors.Is(err, candidate.err) {
			return err.Error(), candidate.kind
		}
	}
	return err.Error(), "unexpected"
}

func evaluateCapabilityCase(t *testing.T, c capabilityCase) capabilityCase {
	t.Helper()
	master, err := hex.DecodeString(c.MasterHex)
	if err != nil {
		t.Fatal(err)
	}
	var output any
	switch c.Kind {
	case "derive":
		var derived []byte
		derived, err = DeriveKey(master)
		output = hex.EncodeToString(derived)
	case "sign":
		var derived []byte
		derived, err = DeriveKey(master)
		if err == nil {
			var token Token
			token, err = sign(derived, *c.Claims)
			output = token.Encode()
		}
	case "decode":
		var token Token
		token, err = Decode(c.Token)
		output = token.Encode()
	case "verify":
		output, err = NewVerifier(master).Verify(c.Token)
	case "scope":
		output = struct {
			Denied  bool `json:"deny_control_plane"`
			InScope bool `json:"in_scope"`
		}{DenyControlPlane(c.Target), InScope(c.Scope, c.Target)}
	case "ceiling":
		output = policy.ScopeCeiling(c.Scope, c.Target, *c.Result)
	default:
		t.Fatalf("unknown case kind %s", c.Kind)
	}
	if err != nil {
		c.Error, c.ErrorKind = oracleError(err)
	} else {
		c.Output = oracleJSON(t, output)
	}
	return c
}

func oracleRoot(t *testing.T) string {
	t.Helper()
	_, source, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("cannot locate oracle source")
	}
	return filepath.Clean(filepath.Join(filepath.Dir(source), "../../.."))
}

func pinnedSourceMatches(current, pinned []byte) bool { return bytes.Equal(current, pinned) }

func oracleProvenance(t *testing.T, root string) (map[string]string, map[string]string) {
	t.Helper()
	sources, generators := map[string]string{}, map[string]string{}
	for _, path := range []string{
		"guard/internal/capability/deny.go", "guard/internal/capability/key.go",
		"guard/internal/capability/token.go", "guard/internal/policy/scope.go",
	} {
		current, err := os.ReadFile(filepath.Join(root, path))
		if err != nil {
			t.Fatal(err)
		}
		command := exec.Command("git", "show", capabilityOracleRevision+":"+path)
		command.Dir = root
		pinned, err := command.Output()
		if err != nil {
			t.Fatal(err)
		}
		if !pinnedSourceMatches(current, pinned) {
			t.Fatalf("oracle source drift: %s", path)
		}
		sources[path] = fmt.Sprintf("%x", sha256.Sum256(current))
	}
	for _, path := range []string{"oracle_test.go", "oracle_cases_test.go"} {
		data, err := os.ReadFile(filepath.Join(root, "guard/internal/capability", path))
		if err != nil {
			t.Fatal(err)
		}
		generators[path] = fmt.Sprintf("%x", sha256.Sum256(data))
	}
	return sources, generators
}

func fixtureMatches(actual, expected []byte) bool { return bytes.Equal(actual, expected) }

// The bubble fixes production time.Now at 2000-01-01T00:00:00Z. Signing,
// verification, decoding, derivation and scope all execute the pinned Go code.
func TestCapabilityOracleFixture(t *testing.T) {
	root := oracleRoot(t)
	sources, generators := oracleProvenance(t, root)
	synctest.Test(t, func(t *testing.T) {
		suite := capabilitySuite{OracleRevision: capabilityOracleRevision, SourceSHA256: sources, GeneratorSHA256: generators}
		seen := map[string]bool{}
		for _, c := range capabilityCases(t, time.Now().Unix()) {
			if c.ID == "" || seen[c.ID] {
				t.Fatalf("empty or duplicate case ID: %s", c.ID)
			}
			seen[c.ID] = true
			suite.Cases = append(suite.Cases, evaluateCapabilityCase(t, c))
			fmt.Printf("CAPABILITY_CASE_ID=%s\n", c.ID)
		}
		if len(suite.Cases) == 0 {
			t.Fatal("zero oracle cases")
		}
		data, err := json.MarshalIndent(suite, "", "  ")
		if err != nil {
			t.Fatal(err)
		}
		data = append(data, '\n')
		path := os.Getenv("SYMBRAIN_CAPABILITY_FIXTURE")
		if path == "" {
			path = filepath.Join(root, "rust/symbrain-guard-core/tests/fixtures/capability_oracle.json")
		}
		if os.Getenv("SYMBRAIN_CAPABILITY_UPDATE") == "1" {
			if err := os.WriteFile(path, data, 0o644); err != nil {
				t.Fatal(err)
			}
		} else {
			expected, err := os.ReadFile(path)
			if err != nil {
				t.Fatal(err)
			}
			if !fixtureMatches(data, expected) {
				t.Fatal("capability oracle fixture drift; regenerate deliberately")
			}
		}
		t.Logf("%d pinned Go capability cases, zero drift", len(suite.Cases))
	})
}

func TestCapabilityOracleRejectsDrift(t *testing.T) {
	for _, check := range []struct {
		name    string
		matches func([]byte, []byte) bool
	}{{"source", pinnedSourceMatches}, {"fixture", fixtureMatches}} {
		t.Run(check.name, func(t *testing.T) {
			original := []byte(`{"output":"allow"}`)
			mutated := []byte(`{"output":"deny"}`)
			if !check.matches(original, original) || check.matches(mutated, original) {
				t.Fatal("drift rejection does not distinguish mutated bytes")
			}
		})
	}
}
