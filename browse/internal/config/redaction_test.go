package config

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestConfigShowOmitsEncryptionKeyMaterial(t *testing.T) {
	corpus := readRedactionCorpus(t)
	marker := corpus.SecretValues[len(corpus.SecretValues)-1]
	t.Setenv("SYMBROWSE_ENCRYPTION_KEY", marker)
	t.Setenv("SYMBROWSE_CDP_ENDPOINT", corpus.Endpoint)
	t.Setenv("HOME", t.TempDir())
	t.Chdir(t.TempDir())
	result, err := LoadWithOverrides(FlagOverrides{})
	if err != nil {
		t.Fatal(err)
	}
	jsonOutput, err := json.Marshal(ShowOutputFor(result))
	if err != nil {
		t.Fatal(err)
	}
	var text strings.Builder
	if err := WriteShow(&text, result, false); err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(jsonOutput), marker) || strings.Contains(text.String(), marker) {
		t.Fatal("config show exposed encryption key material")
	}
	for _, secret := range corpus.SecretValues {
		if strings.Contains(string(jsonOutput), secret) || strings.Contains(text.String(), secret) {
			t.Fatalf("config show exposed synthetic secret marker %q", secret)
		}
	}
	if !strings.Contains(string(jsonOutput), "127.0.0.1:9222") || !strings.Contains(string(jsonOutput), "mode=active") {
		t.Fatal("config show redacted safe endpoint details")
	}
}

type redactionCorpus struct {
	SecretValues []string `json:"secret_values"`
	Endpoint     string   `json:"endpoint"`
}

func readRedactionCorpus(t *testing.T) redactionCorpus {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("..", "..", "testdata", "port", "security", "redaction-corpus.json"))
	if err != nil {
		t.Fatal(err)
	}
	var corpus redactionCorpus
	if err := json.Unmarshal(data, &corpus); err != nil {
		t.Fatal(err)
	}
	return corpus
}
