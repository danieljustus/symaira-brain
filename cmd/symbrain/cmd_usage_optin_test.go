package main

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/output"
	"github.com/danieljustus/symaira-brain/internal/usage"
)

func TestCmdUsageOptInBackend(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("GROK_HOME", home)
	t.Setenv("PATH", home)
	marker := filepath.Join(home, "accessed")
	for _, name := range []string{"symvault", "security", "ps", "lsof", "gh"} {
		if err := os.WriteFile(filepath.Join(home, name), []byte("#!/bin/sh\nprintf accessed >> '"+marker+"'\nexit 1\n"), 0700); err != nil {
			t.Fatal(err)
		}
	}
	for _, key := range envKeysForProviders {
		t.Setenv(key, "symvault://test/excluded")
	}
	for _, key := range []string{"ZAI_API_KEY", "DEVIN_API_KEY", "GROK_ACCESS_TOKEN"} {
		t.Setenv(key, "")
	}
	for _, id := range []string{"zai", "devin", "grok", "ollama"} {
		var stdout, stderr bytes.Buffer
		code := cmdUsageWithFormat([]string{"--providers", id}, &stdout, &stderr, output.FormatJSON)
		if code != 0 {
			t.Fatalf("%s code %d: %s", id, code, stderr.String())
		}
		var report usage.Report
		if err := json.Unmarshal(stdout.Bytes(), &report); err != nil {
			t.Fatal(err)
		}
		if len(report.Providers) != 1 || report.Providers[0].ID != id {
			t.Fatalf("unexpected report: %+v", report)
		}
		p := report.Providers[0]
		if id == "ollama" {
			if !p.Configured || p.Error == "" || p.Snapshot != nil {
				t.Fatalf("missing signing key must fail closed: %+v", p)
			}
		} else if p.Configured {
			t.Fatalf("unexpected configured provider: %+v", p)
		}
	}
	for _, key := range []string{"ZAI_API_KEY", "DEVIN_API_KEY", "GROK_ACCESS_TOKEN"} {
		t.Setenv(key, "symvault://test/must-not-resolve")
	}
	for _, selector := range []string{"zai,unknown", "devin,unknown", "grok,unknown", "ollama,unknown"} {
		var stdout, stderr bytes.Buffer
		if code := cmdUsageWithFormat([]string{"--providers", selector}, &stdout, &stderr, output.FormatJSON); code != 2 || stdout.Len() != 0 {
			t.Fatalf("%s: code %d stdout %s", selector, code, stdout.String())
		}
	}
	if _, err := os.Stat(marker); !os.IsNotExist(err) {
		t.Fatalf("excluded credential/process tool executed: %v", err)
	}
}
