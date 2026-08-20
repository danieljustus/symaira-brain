package config

import (
	"os"
	"path/filepath"
	"testing"
)

func writeTempConfig(t *testing.T, dir, name, content string) string {
	t.Helper()
	path := filepath.Join(dir, name)
	if err := os.WriteFile(path, []byte(content), 0644); err != nil {
		t.Fatalf("failed to write %s: %v", name, err)
	}
	return path
}

func TestDefaults(t *testing.T) {
	cfg := Defaults()
	if cfg.Ollama.URL != "http://localhost:11434/api/embeddings" {
		t.Errorf("expected default Ollama URL, got %q", cfg.Ollama.URL)
	}
	if cfg.Ollama.Model != "nomic-embed-text" {
		t.Errorf("expected default Ollama model, got %q", cfg.Ollama.Model)
	}
	if cfg.Security.PIIEnabled == nil || !*cfg.Security.PIIEnabled {
		t.Error("expected PII enabled by default")
	}
	if cfg.QueryLog.MaxEntries != 1000 {
		t.Errorf("expected default query log max_entries 1000, got %d", cfg.QueryLog.MaxEntries)
	}
	if cfg.QueryLog.MaxAge != "" {
		t.Errorf("expected default query log max_age empty, got %q", cfg.QueryLog.MaxAge)
	}
}

func TestMergeFileOverrides(t *testing.T) {
	resetCache()
	dir := t.TempDir()
	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", dir)
	defer os.Setenv("HOME", oldHome)

	configDir := filepath.Join(dir, ".config", "symmemory")
	if err := os.MkdirAll(configDir, 0755); err != nil {
		t.Fatalf("failed to create config dir: %v", err)
	}
	writeTempConfig(t, configDir, "config.toml", `
[database]
path = "/custom/db/path.db"
[ollama]
url = "http://ollama:11434/api/embeddings"
model = "llama3"
[jwt]
secret_path = "/etc/secrets/jwt.key"
[server]
http_port = 9090
[security]
pii_enabled = false
`)

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load failed: %v", err)
	}

	if cfg.Database.Path != "/custom/db/path.db" {
		t.Errorf("expected database.path override, got %q", cfg.Database.Path)
	}
	if cfg.Ollama.URL != "http://ollama:11434/api/embeddings" {
		t.Errorf("expected ollama.url override, got %q", cfg.Ollama.URL)
	}
	if cfg.Ollama.Model != "llama3" {
		t.Errorf("expected ollama.model override, got %q", cfg.Ollama.Model)
	}
	if cfg.JWT.SecretPath != "/etc/secrets/jwt.key" {
		t.Errorf("expected jwt.secret_path override, got %q", cfg.JWT.SecretPath)
	}
	if cfg.Server.HTTPPort != 9090 {
		t.Errorf("expected server.http_port override, got %d", cfg.Server.HTTPPort)
	}
	if cfg.Security.PIIEnabled == nil || *cfg.Security.PIIEnabled {
		t.Error("expected PII disabled after override")
	}
}

func TestMergeFileMissing(t *testing.T) {
	resetCache()
	dir := t.TempDir()
	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", dir)
	defer os.Setenv("HOME", oldHome)

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load failed for empty home: %v", err)
	}
	if cfg.Ollama.URL != "http://localhost:11434/api/embeddings" {
		t.Errorf("expected default URL preserved, got %q", cfg.Ollama.URL)
	}
}

func TestMergeFilePartial(t *testing.T) {
	resetCache()
	dir := t.TempDir()
	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", dir)
	defer os.Setenv("HOME", oldHome)

	configDir := filepath.Join(dir, ".config", "symmemory")
	if err := os.MkdirAll(configDir, 0755); err != nil {
		t.Fatalf("failed to create config dir: %v", err)
	}
	writeTempConfig(t, configDir, "config.toml", `
[ollama]
model = "custom-model"
`)

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load failed: %v", err)
	}

	if cfg.Ollama.Model != "custom-model" {
		t.Errorf("expected model override, got %q", cfg.Ollama.Model)
	}
	if cfg.Ollama.URL != "http://localhost:11434/api/embeddings" {
		t.Errorf("expected default URL preserved, got %q", cfg.Ollama.URL)
	}
}

func TestLoadGlobalConfig(t *testing.T) {
	resetCache()
	dir := t.TempDir()
	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", dir)
	defer os.Setenv("HOME", oldHome)

	configDir := filepath.Join(dir, ".config", "symmemory")
	if err := os.MkdirAll(configDir, 0755); err != nil {
		t.Fatalf("failed to create config dir: %v", err)
	}
	writeTempConfig(t, configDir, "config.toml", `
[ollama]
url = "http://custom:1234/api/embeddings"
[server]
http_port = 8080
`)

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load failed: %v", err)
	}

	if cfg.Ollama.URL != "http://custom:1234/api/embeddings" {
		t.Errorf("expected global ollama.url, got %q", cfg.Ollama.URL)
	}
	if cfg.Server.HTTPPort != 8080 {
		t.Errorf("expected global server.http_port, got %d", cfg.Server.HTTPPort)
	}
}

func TestLoadProjectOverride(t *testing.T) {
	resetCache()
	dir := t.TempDir()
	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", dir)
	defer os.Setenv("HOME", oldHome)

	configDir := filepath.Join(dir, ".config", "symmemory")
	if err := os.MkdirAll(configDir, 0755); err != nil {
		t.Fatalf("failed to create config dir: %v", err)
	}
	writeTempConfig(t, configDir, "config.toml", `
[ollama]
model = "global-model"
`)

	oldCwd, _ := os.Getwd()
	os.Chdir(dir)
	defer os.Chdir(oldCwd)

	writeTempConfig(t, dir, ".symmemory.toml", `
[ollama]
model = "project-model"
`)

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load failed: %v", err)
	}

	if cfg.Ollama.Model != "project-model" {
		t.Errorf("expected project override, got %q", cfg.Ollama.Model)
	}
}

func TestDefaultsArePureGo(t *testing.T) {
	cfg := Defaults()
	_ = cfg.Database.Path
	_ = cfg.Ollama.URL
	_ = cfg.JWT.SecretPath
}

func TestEnvOverrideDBPath(t *testing.T) {
	resetCache()
	dir := t.TempDir()
	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", dir)
	defer os.Setenv("HOME", oldHome)

	os.Setenv("SYMMEMORY_DATABASE_PATH", "/tmp/test.db")
	defer os.Unsetenv("SYMMEMORY_DATABASE_PATH")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load failed: %v", err)
	}
	if cfg.Database.Path != "/tmp/test.db" {
		t.Errorf("expected Database.Path=/tmp/test.db, got %q", cfg.Database.Path)
	}
}

func TestEnvOverrideOllamaURL(t *testing.T) {
	resetCache()
	dir := t.TempDir()
	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", dir)
	defer os.Setenv("HOME", oldHome)

	os.Setenv("SYMMEMORY_OLLAMA_URL", "http://custom:1234/api/embeddings")
	defer os.Unsetenv("SYMMEMORY_OLLAMA_URL")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load failed: %v", err)
	}
	if cfg.Ollama.URL != "http://custom:1234/api/embeddings" {
		t.Errorf("expected Ollama.URL=http://custom:1234/api/embeddings, got %q", cfg.Ollama.URL)
	}
}

func TestEnvOverrideOllamaModel(t *testing.T) {
	resetCache()
	dir := t.TempDir()
	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", dir)
	defer os.Setenv("HOME", oldHome)

	os.Setenv("SYMMEMORY_OLLAMA_MODEL", "llama3")
	defer os.Unsetenv("SYMMEMORY_OLLAMA_MODEL")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load failed: %v", err)
	}
	if cfg.Ollama.Model != "llama3" {
		t.Errorf("expected Ollama.Model=llama3, got %q", cfg.Ollama.Model)
	}
}

func TestReloadReadsFreshConfig(t *testing.T) {
	resetCache()
	dir := t.TempDir()
	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", dir)
	defer os.Setenv("HOME", oldHome)

	configDir := filepath.Join(dir, ".config", "symmemory")
	if err := os.MkdirAll(configDir, 0755); err != nil {
		t.Fatalf("failed to create config dir: %v", err)
	}
	writeTempConfig(t, configDir, "config.toml", `
[server]
http_port = 8080
`)

	cfg1, err := Reload()
	if err != nil {
		t.Fatalf("first Reload failed: %v", err)
	}
	if cfg1.Server.HTTPPort != 8080 {
		t.Errorf("expected port 8080, got %d", cfg1.Server.HTTPPort)
	}

	writeTempConfig(t, configDir, "config.toml", `
[server]
http_port = 9999
`)

	cfg2, err := Reload()
	if err != nil {
		t.Fatalf("second Reload failed: %v", err)
	}
	if cfg2.Server.HTTPPort != 9999 {
		t.Errorf("expected port 9999 after file change, got %d", cfg2.Server.HTTPPort)
	}
}

func TestReloadAppliesEnvVars(t *testing.T) {
	resetCache()
	dir := t.TempDir()
	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", dir)
	defer os.Setenv("HOME", oldHome)

	os.Setenv("SYMMEMORY_DATABASE_PATH", "/tmp/reload.db")
	defer os.Unsetenv("SYMMEMORY_DATABASE_PATH")

	cfg, err := Reload()
	if err != nil {
		t.Fatalf("Reload failed: %v", err)
	}
	if cfg.Database.Path != "/tmp/reload.db" {
		t.Errorf("expected Database.Path=/tmp/reload.db, got %q", cfg.Database.Path)
	}
}

func TestReloadDoesNotAffectCachedLoad(t *testing.T) {
	resetCache()
	dir := t.TempDir()
	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", dir)
	defer os.Setenv("HOME", oldHome)

	configDir := filepath.Join(dir, ".config", "symmemory")
	if err := os.MkdirAll(configDir, 0755); err != nil {
		t.Fatalf("failed to create config dir: %v", err)
	}
	writeTempConfig(t, configDir, "config.toml", `
[server]
http_port = 1111
`)

	cached, err := Load()
	if err != nil {
		t.Fatalf("Load failed: %v", err)
	}
	if cached.Server.HTTPPort != 1111 {
		t.Errorf("expected cached port 1111, got %d", cached.Server.HTTPPort)
	}

	writeTempConfig(t, configDir, "config.toml", `
[server]
http_port = 2222
`)

	reloaded, err := Reload()
	if err != nil {
		t.Fatalf("Reload failed: %v", err)
	}
	if reloaded.Server.HTTPPort != 2222 {
		t.Errorf("expected reloaded port 2222, got %d", reloaded.Server.HTTPPort)
	}

	if cached.Server.HTTPPort != 1111 {
		t.Errorf("cached config was mutated by Reload: expected 1111, got %d", cached.Server.HTTPPort)
	}

	cachedAgain, err := Load()
	if err != nil {
		t.Fatalf("second Load failed: %v", err)
	}
	if cachedAgain.Server.HTTPPort != 1111 {
		t.Errorf("Load() should still return cached value 1111, got %d", cachedAgain.Server.HTTPPort)
	}
}

func TestMergeFileMissingSecuritySection(t *testing.T) {
	resetCache()
	dir := t.TempDir()
	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", dir)
	defer os.Setenv("HOME", oldHome)

	configDir := filepath.Join(dir, ".config", "symmemory")
	if err := os.MkdirAll(configDir, 0755); err != nil {
		t.Fatalf("failed to create config dir: %v", err)
	}
	writeTempConfig(t, configDir, "config.toml", `
[ollama]
model = "custom-model"
`)

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load failed: %v", err)
	}

	if cfg.Ollama.Model != "custom-model" {
		t.Errorf("expected model override, got %q", cfg.Ollama.Model)
	}
	if cfg.Security.PIIEnabled == nil || !*cfg.Security.PIIEnabled {
		t.Error("expected PII enabled when security section is absent from config")
	}
}
