package managed

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"crypto/sha256"
	"encoding/hex"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// --- verifyChecksum additional edge cases ---

func TestVerifyChecksum_MatchWithWhitespace(t *testing.T) {
	content := []byte("test data")
	path := writeTempFile(t, content)
	expected := sha256Sum(content)
	// Checksums.txt often has trailing whitespace.
	if err := verifyChecksum(path, "  "+expected+"  \n"); err != nil {
		t.Errorf("verifyChecksum with whitespace: %v", err)
	}
}

// --- findChecksumInFile additional edge cases ---

func TestFindChecksumInFile_EmptyFile(t *testing.T) {
	path := writeTempFile(t, []byte(""))
	_, err := findChecksumInFile(path, "anything.tar.gz")
	if err == nil {
		t.Error("findChecksumInFile empty file: got nil, want error")
	}
}

func TestFindChecksumInFile_FileNotFound(t *testing.T) {
	_, err := findChecksumInFile("/nonexistent/checksums.txt", "file.tar.gz")
	if err == nil {
		t.Error("findChecksumInFile nonexistent file: got nil, want error")
	}
}

func TestFindChecksumInFile_BlankAndCommentLines(t *testing.T) {
	// Only blank lines and comments — no actual entries.
	content := "# header\n\n# another comment\n\n"
	path := writeTempFile(t, []byte(content))
	_, err := findChecksumInFile(path, "anything.tar.gz")
	if err == nil {
		t.Error("findChecksumInFile with only comments: got nil, want error")
	}
}

// --- extractBinary tests ---

func TestExtractBinary_BinaryNotFound(t *testing.T) {
	archive := buildArchiveForExtract(t, "wrong-binary", []byte("data"))
	path := writeTempFile(t, archive)
	core := &Core{BinaryName: "expected-binary"}
	_, err := extractBinary(path, core, "darwin", "arm64")
	if err == nil {
		t.Error("extractBinary with missing binary: got nil, want error")
	}
	if !strings.Contains(err.Error(), "not found") {
		t.Errorf("error = %q, want it to mention 'not found'", err.Error())
	}
}

func TestExtractBinary_InvalidGzip(t *testing.T) {
	path := writeTempFile(t, []byte("this is not gzip"))
	core := &Core{BinaryName: "binary"}
	_, err := extractBinary(path, core, "darwin", "arm64")
	if err == nil {
		t.Error("extractBinary with invalid gzip: got nil, want error")
	}
	if !strings.Contains(err.Error(), "gzip") {
		t.Errorf("error = %q, want it to mention gzip", err.Error())
	}
}

func TestExtractBinary_FileNotFound(t *testing.T) {
	core := &Core{BinaryName: "binary"}
	_, err := extractBinary("/nonexistent/archive.tar.gz", core, "darwin", "arm64")
	if err == nil {
		t.Error("extractBinary nonexistent file: got nil, want error")
	}
}

// --- atomicInstall tests ---

func TestAtomicInstall_Success(t *testing.T) {
	binDir := t.TempDir()
	data := []byte("#!/bin/sh\necho hello")
	if err := atomicInstall(binDir, "mybin", data); err != nil {
		t.Fatalf("atomicInstall: %v", err)
	}
	got, err := os.ReadFile(filepath.Join(binDir, "mybin"))
	if err != nil {
		t.Fatalf("read installed binary: %v", err)
	}
	if !bytes.Equal(got, data) {
		t.Errorf("installed content = %q, want %q", got, data)
	}
	info, err := os.Stat(filepath.Join(binDir, "mybin"))
	if err != nil {
		t.Fatalf("stat: %v", err)
	}
	if info.Mode().Perm()&0o100 == 0 {
		t.Error("installed binary should be executable")
	}
}

func TestAtomicInstall_NonexistentBinDir(t *testing.T) {
	err := atomicInstall("/nonexistent/dir/that/doesnt/exist", "mybin", []byte("data"))
	if err == nil {
		t.Error("atomicInstall with nonexistent dir: got nil, want error")
	}
}

func TestAtomicInstall_OverwritesExisting(t *testing.T) {
	binDir := t.TempDir()
	// Write an initial binary.
	if err := atomicInstall(binDir, "mybin", []byte("old")); err != nil {
		t.Fatalf("first install: %v", err)
	}
	// Overwrite with new content.
	newData := []byte("#!/bin/sh\necho new")
	if err := atomicInstall(binDir, "mybin", newData); err != nil {
		t.Fatalf("second install: %v", err)
	}
	got, err := os.ReadFile(filepath.Join(binDir, "mybin"))
	if err != nil {
		t.Fatalf("read: %v", err)
	}
	if !bytes.Equal(got, newData) {
		t.Errorf("content = %q, want %q", got, newData)
	}
}

// --- Helpers ---

func sha256Sum(data []byte) string {
	h := sha256.New()
	h.Write(data)
	return hex.EncodeToString(h.Sum(nil))
}

func writeTempFile(t *testing.T, content []byte) string {
	t.Helper()
	f, err := os.CreateTemp(t.TempDir(), "test-*")
	if err != nil {
		t.Fatalf("CreateTemp: %v", err)
	}
	if _, err := f.Write(content); err != nil {
		t.Fatalf("Write: %v", err)
	}
	f.Close()
	return f.Name()
}

func buildArchiveForExtract(t *testing.T, binaryName string, data []byte) []byte {
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
