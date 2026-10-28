package main

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/output"
	"github.com/danieljustus/symaira-brain/internal/usage"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

func TestCmdUsageProviderSelection(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("CODEX_HOME", home)
	marker := filepath.Join(home, "accessed")
	for _, command := range []string{"symvault", "security", "ps", "lsof", "gh"} {
		if err := os.WriteFile(filepath.Join(home, command), []byte("#!/bin/sh\nprintf accessed >> '"+marker+"'\nexit 1\n"), 0700); err != nil {
			t.Fatal(err)
		}
	}
	t.Setenv("PATH", home)
	for _, key := range envKeysForProviders {
		t.Setenv(key, "symvault://test/excluded")
	}
	for _, key := range []string{"ZAI_API_KEY", "DEVIN_API_KEY", "GROK_ACCESS_TOKEN"} {
		t.Setenv(key, "symvault://test/excluded")
	}
	t.Setenv("CODEX_ACCESS_TOKEN", "")
	t.Setenv("OPENROUTER_API_KEY", "")
	for _, args := range [][]string{
		{"--providers", "openrouter,codex,codex"},
		{"--providers=codex,openrouter"},
	} {
		var stdout, stderr bytes.Buffer
		if code := cmdUsageWithFormat(args, &stdout, &stderr, output.FormatJSON); code != exitcodes.ExitOK {
			t.Fatalf("code %d: %s", code, stderr.String())
		}
		var report usage.Report
		if err := json.Unmarshal(stdout.Bytes(), &report); err != nil {
			t.Fatal(err)
		}
		ids := make([]string, len(report.Providers))
		for i, provider := range report.Providers {
			ids[i] = provider.ID
			if provider.Configured {
				t.Fatalf("unexpected configured %s", provider.ID)
			}
		}
		if !reflect.DeepEqual(ids, []string{"codex", "openrouter"}) {
			t.Fatalf("providers: %v", ids)
		}
	}
	t.Setenv("CODEX_ACCESS_TOKEN", "symvault://test/must-not-resolve")
	for _, selector := range []string{"", "unknown", "codex,unknown", "codex,", ",codex"} {
		var stdout, stderr bytes.Buffer
		if code := cmdUsageWithFormat([]string{"--providers", selector}, &stdout, &stderr, output.FormatJSON); code != exitcodes.ExitNoInput || stdout.Len() != 0 {
			t.Fatalf("selector %q: code %d stdout %q stderr %s", selector, code, stdout.String(), stderr.String())
		}
	}
	if _, err := os.Stat(marker); !os.IsNotExist(err) {
		t.Fatalf("excluded credential/process command was accessed: %v", err)
	}
}
