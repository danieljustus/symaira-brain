package main

import (
	"bytes"
	"encoding/json"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-corekit/exitcodes"
)

func TestCmdVersion_HumanOutput(t *testing.T) {
	var stdout, stderr bytes.Buffer

	code := cmdVersion(nil, &stdout, &stderr)

	if code != exitcodes.ExitOK {
		t.Fatalf("cmdVersion() = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}
	if !strings.Contains(stdout.String(), "symbrain "+version) {
		t.Errorf("stdout = %q, want it to contain %q", stdout.String(), "symbrain "+version)
	}
}

func TestCmdVersion_JSONMatchesVersionkitSchema(t *testing.T) {
	var stdout, stderr bytes.Buffer

	code := cmdVersion([]string{"--json"}, &stdout, &stderr)

	if code != exitcodes.ExitOK {
		t.Fatalf("cmdVersion(--json) = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	var payload struct {
		Tool          string `json:"tool"`
		Version       string `json:"version"`
		SchemaVersion int    `json:"schema_version"`
	}
	if err := json.Unmarshal(stdout.Bytes(), &payload); err != nil {
		t.Fatalf("output is not valid JSON: %v (output: %q)", err, stdout.String())
	}

	if payload.Tool != "symbrain" {
		t.Errorf("tool = %q, want %q", payload.Tool, "symbrain")
	}
	if payload.Version != version {
		t.Errorf("version = %q, want %q", payload.Version, version)
	}
	if payload.SchemaVersion != versionSchema {
		t.Errorf("schema_version = %d, want %d", payload.SchemaVersion, versionSchema)
	}
}

func TestCmdVersion_UnknownFlagExitsNoInput(t *testing.T) {
	var stdout, stderr bytes.Buffer

	code := cmdVersion([]string{"--bogus"}, &stdout, &stderr)

	if code != exitcodes.ExitNoInput {
		t.Fatalf("cmdVersion(--bogus) = %d, want %d", code, exitcodes.ExitNoInput)
	}
}
