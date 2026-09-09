package harness

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestReadConfigFileRejectsOversizedRegularFile(t *testing.T) {
	path := filepath.Join(t.TempDir(), "config.json")
	file, err := os.Create(path)
	if err != nil {
		t.Fatalf("Create: %v", err)
	}
	if err := file.Truncate(maxBindingConfigBytes + 1); err != nil {
		file.Close()
		t.Fatalf("Truncate: %v", err)
	}
	if err := file.Close(); err != nil {
		t.Fatalf("Close: %v", err)
	}

	_, err = readConfigFile(path)
	if err == nil || !strings.Contains(err.Error(), "exceeds maximum size") {
		t.Fatalf("readConfigFile oversized error = %v, want size limit error", err)
	}
}
