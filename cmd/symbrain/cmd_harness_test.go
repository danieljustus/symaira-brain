package main

import (
	"bytes"
	"encoding/json"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/harness"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

func TestRunHarnessListJSON(t *testing.T) {
	t.Setenv("HOME", t.TempDir())
	var stdout, stderr bytes.Buffer

	code := run([]string{"--output", "json", "harness", "list"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("run() = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	var report harness.Inventory
	if err := json.Unmarshal(stdout.Bytes(), &report); err != nil {
		t.Fatalf("JSON output: %v (%q)", err, stdout.String())
	}
	if report.SchemaVersion != harness.InventorySchemaVersion {
		t.Fatalf("schema_version = %d, want %d", report.SchemaVersion, harness.InventorySchemaVersion)
	}
	if len(report.Harnesses) != len(harness.All) {
		t.Fatalf("harness count = %d, want %d", len(report.Harnesses), len(harness.All))
	}
}

func TestRunHarnessListProjectJSON(t *testing.T) {
	t.Setenv("HOME", t.TempDir())
	var stdout, stderr bytes.Buffer

	code := run([]string{"harness", "list", "--project", t.TempDir(), "--json"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("run() = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}
	if !bytes.Contains(stdout.Bytes(), []byte(`"project_dir"`)) {
		t.Fatalf("JSON output missing project_dir: %s", stdout.String())
	}
}
