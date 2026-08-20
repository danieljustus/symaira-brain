package config

import (
	"os"
	"path/filepath"
	"testing"
)

func TestWorkingMemoryDefaults(t *testing.T) {
	cfg := Defaults()

	if cfg.WorkingMemory.TTL != "24h" {
		t.Errorf("expected WorkingMemory.TTL=24h, got %q", cfg.WorkingMemory.TTL)
	}
	if cfg.WorkingMemory.MaxItems != 50 {
		t.Errorf("expected WorkingMemory.MaxItems=50, got %d", cfg.WorkingMemory.MaxItems)
	}
	if !cfg.WorkingMemory.IncludeInContext {
		t.Error("expected WorkingMemory.IncludeInContext=true by default")
	}
}

func TestWorkingMemoryEnvOverrideString(t *testing.T) {
	resetCache()
	dir := t.TempDir()
	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", dir)
	defer os.Setenv("HOME", oldHome)

	os.Setenv("SYMMEMORY_WORKING_MEMORY_TTL", "12h")
	defer os.Unsetenv("SYMMEMORY_WORKING_MEMORY_TTL")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load failed: %v", err)
	}
	if cfg.WorkingMemory.TTL != "12h" {
		t.Errorf("expected env-overridden TTL=12h, got %q", cfg.WorkingMemory.TTL)
	}
}

func TestWorkingMemoryTomlOverride(t *testing.T) {
	resetCache()
	dir := t.TempDir()
	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", dir)
	defer os.Setenv("HOME", oldHome)

	configDir := filepath.Join(dir, ".config", "symmemory")
	if err := os.MkdirAll(configDir, 0o700); err != nil {
		t.Fatalf("MkdirAll failed: %v", err)
	}
	if err := os.WriteFile(configDir+"/config.toml", []byte(`[working_memory]
ttl = "48h"
max_items = 10
`), 0o600); err != nil {
		t.Fatalf("WriteFile failed: %v", err)
	}

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load failed: %v", err)
	}
	if cfg.WorkingMemory.TTL != "48h" {
		t.Errorf("expected TOML-overridden TTL=48h, got %q", cfg.WorkingMemory.TTL)
	}
	if cfg.WorkingMemory.MaxItems != 10 {
		t.Errorf("expected TOML-overridden MaxItems=10, got %d", cfg.WorkingMemory.MaxItems)
	}
}
