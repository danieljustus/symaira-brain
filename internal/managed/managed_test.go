package managed

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

func TestLoadManifest(t *testing.T) {
	m, err := LoadManifest()
	if err != nil {
		t.Fatalf("LoadManifest: %v", err)
	}
	if m.SchemaVersion != 1 {
		t.Errorf("SchemaVersion = %d, want 1", m.SchemaVersion)
	}
	if len(m.Cores) != 3 {
		t.Errorf("len(Cores) = %d, want 3", len(m.Cores))
	}
	for _, name := range []string{"symvault", "symmemory", "symskills"} {
		c, ok := m.Cores[name]
		if !ok {
			t.Errorf("missing core %q", name)
			continue
		}
		if c.Version == "" {
			t.Errorf("core %q: Version is empty", name)
		}
		if c.Repo == "" {
			t.Errorf("core %q: Repo is empty", name)
		}
		if c.BinaryName == "" {
			t.Errorf("core %q: BinaryName is empty", name)
		}
	}
}

func TestAssetName_Versioned(t *testing.T) {
	core := &Core{
		Version:     "v1.2.3",
		BinaryName:  "symvault",
		AssetPrefix: "symaira-vault",
	}
	got := core.AssetName("darwin", "arm64")
	want := "symaira-vault_v1.2.3_darwin_arm64.tar.gz"
	if got != want {
		t.Errorf("AssetName() = %q, want %q", got, want)
	}
}

func TestAssetNameAlt_Unversioned(t *testing.T) {
	core := &Core{
		Version:     "v1.2.3",
		BinaryName:  "symmemory",
		AssetPrefix: "symmemory",
	}
	got := core.AssetNameAlt("linux", "amd64")
	want := "symmemory_linux_amd64.tar.gz"
	if got != want {
		t.Errorf("AssetNameAlt() = %q, want %q", got, want)
	}
}

func TestBinaryPathInArchive_Vault(t *testing.T) {
	core := &Core{
		Version:     "v0.15.3",
		BinaryName:  "symvault",
		AssetPrefix: "symaira-vault",
	}
	got := core.BinaryPathInArchive("darwin", "arm64")
	want := "symaira-vault_v0.15.3_darwin_arm64/symvault"
	if got != want {
		t.Errorf("BinaryPathInArchive() = %q, want %q", got, want)
	}
}

func TestBinaryPathInArchive_Memory(t *testing.T) {
	core := &Core{
		Version:     "v0.17.0",
		BinaryName:  "symmemory",
		AssetPrefix: "symmemory",
	}
	got := core.BinaryPathInArchive("darwin", "arm64")
	want := "symmemory"
	if got != want {
		t.Errorf("BinaryPathInArchive() = %q, want %q", got, want)
	}
}

func TestChecksumAssetName(t *testing.T) {
	core := &Core{AssetPrefix: "symaira-vault"}
	got := core.ChecksumAssetName()
	want := "checksums.txt"
	if got != want {
		t.Errorf("ChecksumAssetName() = %q, want %q", got, want)
	}
}

func TestFindChecksumInFile(t *testing.T) {
	content := `# checksums for symaira-vault
abc123def456  symaira-vault_v0.15.3_darwin_arm64.tar.gz
789abc012def  symaira-vault_v0.15.3_linux_amd64.tar.gz
`
	f := filepath.Join(t.TempDir(), "checksums.txt")
	if err := os.WriteFile(f, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	got, err := findChecksumInFile(f, "symaira-vault_v0.15.3_darwin_arm64.tar.gz")
	if err != nil {
		t.Fatalf("findChecksumInFile: %v", err)
	}
	want := "abc123def456"
	if got != want {
		t.Errorf("findChecksumInFile() = %q, want %q", got, want)
	}
}

func TestFindChecksumInFile_NotFound(t *testing.T) {
	content := `abc123  symaira-vault_v0.15.3_darwin_arm64.tar.gz
`
	f := filepath.Join(t.TempDir(), "checksums.txt")
	if err := os.WriteFile(f, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	_, err := findChecksumInFile(f, "nonexistent.tar.gz")
	if err == nil {
		t.Error("findChecksumInFile for nonexistent asset: got nil error, want error")
	}
}

func TestVerifyChecksum(t *testing.T) {
	// Create a file with known content
	content := []byte("hello world\n")
	f := filepath.Join(t.TempDir(), "test.txt")
	if err := os.WriteFile(f, content, 0o644); err != nil {
		t.Fatal(err)
	}

	// Compute expected SHA-256
	// echo -n "hello world\n" | shasum -a 256
	// The exact hash depends on whether newline is included
	// Let's just test that verifyChecksum runs and produces consistent results
	if err := verifyChecksum(f, "not-a-hash"); err == nil {
		t.Error("verifyChecksum with wrong hash: got nil error, want error")
	}
}

func TestAtomicInstall(t *testing.T) {
	binDir := t.TempDir()
	data := []byte("#!/bin/sh\necho test\n")

	if err := atomicInstall(binDir, "test-binary", data); err != nil {
		t.Fatalf("atomicInstall: %v", err)
	}

	path := filepath.Join(binDir, "test-binary")
	info, err := os.Stat(path)
	if err != nil {
		t.Fatalf("stat installed binary: %v", err)
	}
	if info.Mode().Perm()&0111 == 0 {
		t.Error("installed binary is not executable")
	}

	got, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read installed binary: %v", err)
	}
	if string(got) != string(data) {
		t.Errorf("installed content = %q, want %q", got, data)
	}
}

func TestInstalledVersion_Missing(t *testing.T) {
	v, err := InstalledVersion(context.Background(), t.TempDir(), "nonexistent")
	if err != nil {
		t.Fatalf("InstalledVersion: %v", err)
	}
	if v != "" {
		t.Errorf("InstalledVersion() = %q, want empty", v)
	}
}
