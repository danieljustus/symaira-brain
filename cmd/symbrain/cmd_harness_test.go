package main

import (
	"bytes"
	"encoding/json"
	"strings"
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
	want := 0
	for _, h := range harness.All {
		if h.SupportsMCPInstall {
			want++
		}
	}
	if len(report.Harnesses) != want {
		t.Fatalf("harness count = %d, want %d", len(report.Harnesses), want)
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

func TestRunHarness_NoArgsPrintsUsage(t *testing.T) {
	var stdout, stderr bytes.Buffer
	code := run([]string{"harness"}, &stdout, &stderr)
	if code != exitcodes.ExitNoInput {
		t.Fatalf("run(harness) = %d, want %d", code, exitcodes.ExitNoInput)
	}
	if !strings.Contains(stderr.String(), "Usage:") {
		t.Errorf("stderr missing usage text: %q", stderr.String())
	}
}

func TestRunHarness_UnknownSubcommandExitsNoInput(t *testing.T) {
	var stdout, stderr bytes.Buffer
	code := run([]string{"harness", "bogus"}, &stdout, &stderr)
	if code != exitcodes.ExitNoInput {
		t.Fatalf("run(harness bogus) = %d, want %d", code, exitcodes.ExitNoInput)
	}
	// Unknown subcommands print usage and exit; they do not produce a
	// separate "unknown subcommand" line (unlike cmdProfile).
	if !strings.Contains(stderr.String(), "Usage:") {
		t.Errorf("stderr missing usage text: %q", stderr.String())
	}
}

func TestRunHarness_HealthDispatch(t *testing.T) {
	t.Setenv("HOME", t.TempDir())
	fake := buildFakeMCP(t)
	writeHealthConfig(t, fake, "")

	var stdout, stderr bytes.Buffer
	code := run([]string{"harness", "health", "--json"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("run(harness health) = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}
	var report harnessHealthReport
	if err := json.Unmarshal(stdout.Bytes(), &report); err != nil {
		t.Fatalf("decode health report: %v (%q)", err, stdout.String())
	}
	if len(report.Servers) != 2 {
		t.Fatalf("servers = %d, want 2", len(report.Servers))
	}
}
