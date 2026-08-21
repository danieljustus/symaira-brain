package version

import (
	"bytes"
	"encoding/json"
	"strings"
	"testing"
)

func TestHasFlag(t *testing.T) {
	tests := []struct {
		name string
		args []string
		flag string
		want bool
	}{
		{"empty args", nil, "--json", false},
		{"present", []string{"--json"}, "--json", true},
		{"present among others", []string{"--verbose", "--json", "x"}, "--json", true},
		{"absent", []string{"--verbose"}, "--json", false},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := hasFlag(tt.args, tt.flag); got != tt.want {
				t.Errorf("hasFlag(%v, %q) = %v, want %v", tt.args, tt.flag, got, tt.want)
			}
		})
	}
}

func TestBuildTime(t *testing.T) {
	out := buildTime()
	if !strings.Contains(out, "compile-time placeholder") {
		t.Errorf("buildTime() = %q, want compile-time placeholder", out)
	}
	if !strings.Contains(out, "2026-01-01") {
		t.Errorf("buildTime() = %q, want placeholder date", out)
	}
}

func TestRun_PlainOutput(t *testing.T) {
	old := version
	t.Cleanup(func() { version = old })
	version = "1.2.3-test"

	var buf bytes.Buffer
	Run(nil, &buf)
	out := buf.String()
	for _, want := range []string{"symguard", "1.2.3-test", "go", "os/arch"} {
		if !strings.Contains(out, want) {
			t.Errorf("Run() output missing %q:\n%s", want, out)
		}
	}
}

func TestRun_JSONContract(t *testing.T) {
	// The version --json payload is the versionkit handshake contract
	// (SchemaVersion 1) consumed by GUI tools. Lock the shape in.
	old := version
	t.Cleanup(func() { version = old })
	version = "9.9.9"

	var buf bytes.Buffer
	Run([]string{"--json"}, &buf)

	var payload struct {
		Tool          string `json:"tool"`
		Version       string `json:"version"`
		SchemaVersion int    `json:"schema_version"`
	}
	if err := json.Unmarshal(buf.Bytes(), &payload); err != nil {
		t.Fatalf("Run(--json) is not valid JSON: %v\n%s", err, buf.String())
	}
	if payload.Tool != "symguard" || payload.Version != "9.9.9" || payload.SchemaVersion != 1 {
		t.Errorf("payload = %+v, want {tool:symguard version:9.9.9 schema_version:1}", payload)
	}
}

func TestSetVersion(t *testing.T) {
	old := version
	t.Cleanup(func() { version = old })

	SetVersion("7.7.7")
	if version != "7.7.7" {
		t.Errorf("SetVersion() = %q, want 7.7.7", version)
	}
}

func TestCheckLatest_DevIsNoop(t *testing.T) {
	// The dev build must never touch the network; CheckLatest must return
	// immediately and without panicking.
	old := version
	t.Cleanup(func() { version = old })

	SetVersion("dev")
	CheckLatest()
}
