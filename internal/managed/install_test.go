package managed

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func buildArchive(t *testing.T, binaryName string, data []byte) []byte {
	t.Helper()
	var buf bytes.Buffer
	gz := gzip.NewWriter(&buf)
	tw := tar.NewWriter(gz)
	if err := tw.WriteHeader(&tar.Header{Name: binaryName, Mode: 0o755, Size: int64(len(data))}); err != nil {
		t.Fatalf("tar header: %v", err)
	}
	if _, err := tw.Write(data); err != nil {
		t.Fatalf("tar write: %v", err)
	}
	if err := tw.Close(); err != nil {
		t.Fatalf("tar close: %v", err)
	}
	if err := gz.Close(); err != nil {
		t.Fatalf("gzip close: %v", err)
	}
	return buf.Bytes()
}

func TestNewInstaller(t *testing.T) {
	inst := NewInstaller("/tmp/example-bin")
	if inst.BinDir != "/tmp/example-bin" {
		t.Errorf("BinDir = %q, want /tmp/example-bin", inst.BinDir)
	}
	if inst.TempDir != "" {
		t.Errorf("TempDir = %q, want empty (system default)", inst.TempDir)
	}
}

func TestVerifyCosign_NotInstalled(t *testing.T) {
	// Hide cosign from PATH so the graceful-degradation branch runs
	// deterministically regardless of what is installed on this machine.
	t.Setenv("PATH", t.TempDir())

	if err := verifyCosign(context.Background(), "/nonexistent/artifact"); err != nil {
		t.Errorf("verifyCosign with no cosign on PATH: got %v, want nil (graceful skip)", err)
	}
}

// TestInstall_AgainstTestServer proves Install/downloadAndVerify/extractBinary
// can be exercised against an httptest.Server via the injectable baseURL,
// without touching the real network.
func TestInstall_AgainstTestServer(t *testing.T) {
	goos, goarch, err := Platform()
	if err != nil {
		t.Fatalf("Platform: %v", err)
	}

	core := &Core{
		Version:     "v1.0.0",
		Repo:        "example/example-core",
		BinaryName:  "example-core",
		AssetPrefix: "example-core",
	}
	binaryData := []byte("#!/bin/sh\necho example\n")
	archive := buildArchive(t, core.BinaryName, binaryData)
	checksum := sha256.Sum256(archive)
	checksumHex := hex.EncodeToString(checksum[:])

	assetName := core.AssetName(goos, goarch)
	checksumsContent := fmt.Sprintf("%s  %s\n", checksumHex, assetName)

	mux := http.NewServeMux()
	mux.HandleFunc("/"+core.Repo+"/releases/latest/download/"+assetName, func(w http.ResponseWriter, r *http.Request) {
		w.Write(archive)
	})
	mux.HandleFunc("/"+core.Repo+"/releases/latest/download/checksums.txt", func(w http.ResponseWriter, r *http.Request) {
		w.Write([]byte(checksumsContent))
	})
	server := httptest.NewServer(mux)
	defer server.Close()

	binDir := t.TempDir()
	inst := &Installer{BinDir: binDir, baseURL: server.URL}

	if err := inst.Install(context.Background(), core); err != nil {
		t.Fatalf("Install: %v", err)
	}

	got, err := os.ReadFile(filepath.Join(binDir, core.BinaryName))
	if err != nil {
		t.Fatalf("read installed binary: %v", err)
	}
	if !bytes.Equal(got, binaryData) {
		t.Errorf("installed content = %q, want %q", got, binaryData)
	}
}

// TestInstall_ChecksumMismatch verifies that a checksum mismatch aborts the
// install and leaves no partial binary behind.
func TestInstall_ChecksumMismatch(t *testing.T) {
	goos, goarch, err := Platform()
	if err != nil {
		t.Fatalf("Platform: %v", err)
	}

	core := &Core{
		Version:     "v1.0.0",
		Repo:        "example/example-core",
		BinaryName:  "example-core",
		AssetPrefix: "example-core",
	}
	archive := buildArchive(t, core.BinaryName, []byte("data"))
	assetName := core.AssetName(goos, goarch)
	// Deliberately wrong checksum.
	checksumsContent := fmt.Sprintf("%s  %s\n", strings.Repeat("0", 64), assetName)

	mux := http.NewServeMux()
	mux.HandleFunc("/"+core.Repo+"/releases/latest/download/"+assetName, func(w http.ResponseWriter, r *http.Request) {
		w.Write(archive)
	})
	mux.HandleFunc("/"+core.Repo+"/releases/latest/download/checksums.txt", func(w http.ResponseWriter, r *http.Request) {
		w.Write([]byte(checksumsContent))
	})
	server := httptest.NewServer(mux)
	defer server.Close()

	binDir := t.TempDir()
	inst := &Installer{BinDir: binDir, baseURL: server.URL}

	if err := inst.Install(context.Background(), core); err == nil {
		t.Fatal("Install with mismatched checksum: got nil error, want error")
	}

	if _, err := os.Stat(filepath.Join(binDir, core.BinaryName)); !os.IsNotExist(err) {
		t.Errorf("binary should not be installed after checksum mismatch, stat err = %v", err)
	}
}
