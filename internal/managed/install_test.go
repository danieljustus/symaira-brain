package managed

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

// cosignCore returns a Core configured like symvault (HasCosign: true)
// for use against a local httptest.Server.
func cosignCore() *Core {
	return &Core{
		Version:     "v1.0.0",
		Repo:        "example/example-core",
		BinaryName:  "example-core",
		AssetPrefix: "example-core",
		HasCosign:   true,
	}
}

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

func TestVerifyCosign_NotInstalled_FailsClosed(t *testing.T) {
	// Hide cosign from PATH so the fail-closed branch runs
	// deterministically regardless of what is installed on this machine.
	t.Setenv("PATH", t.TempDir())

	err := verifyCosign(context.Background(), "/nonexistent/artifact", cosignCore(), false, io.Discard)
	if err == nil {
		t.Fatal("verifyCosign with no cosign on PATH and allowUnsigned=false: got nil, want error (fail closed)")
	}
	if !strings.Contains(err.Error(), "cannot verify publisher signature") {
		t.Errorf("error = %q, want it to explain the publisher could not be verified", err.Error())
	}
}

func TestVerifyCosign_NotInstalled_AllowUnsignedSkipsWithWarning(t *testing.T) {
	t.Setenv("PATH", t.TempDir())

	var warn bytes.Buffer
	if err := verifyCosign(context.Background(), "/nonexistent/artifact", cosignCore(), true, &warn); err != nil {
		t.Errorf("verifyCosign with allowUnsigned=true: got %v, want nil (explicit skip)", err)
	}
	if !strings.Contains(warn.String(), "WARNING") {
		t.Errorf("warn output = %q, want a WARNING about skipped verification", warn.String())
	}
}

func TestVerifyCosign_MissingSignatureFile_FailsClosed(t *testing.T) {
	// cosign itself is on PATH (or not) — either way, a missing .sig
	// must fail closed by default; this only exercises the branch
	// meaningfully when cosign is actually installed, so skip otherwise.
	if _, err := exec.LookPath("cosign"); err != nil {
		t.Skip("cosign not installed on this machine; covered by TestVerifyCosign_NotInstalled_FailsClosed instead")
	}

	dir := t.TempDir()
	artifact := filepath.Join(dir, "artifact.tar.gz")
	if err := os.WriteFile(artifact, []byte("data"), 0o644); err != nil {
		t.Fatalf("write artifact: %v", err)
	}
	// No .sig/.pem written alongside it.

	err := verifyCosign(context.Background(), artifact, cosignCore(), false, io.Discard)
	if err == nil {
		t.Fatal("verifyCosign with missing .sig: got nil, want error (fail closed)")
	}
	if !strings.Contains(err.Error(), "signature file missing") {
		t.Errorf("error = %q, want it to mention the missing signature file", err.Error())
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

// serveCore starts an httptest.Server that serves an archive and
// matching checksums.txt for core, mimicking the GitHub
// "releases/latest/download/<asset>" layout downloadURL constructs.
// The archive (and sig/pem, when non-nil) are mirrored at both
// assetName and altName: Install retries with the unversioned
// AssetNameAlt whenever the first attempt errors — for these tests
// that "first attempt" deliberately fails downstream of the download
// (bad checksum, missing signature), and without the mirror that retry
// would hit an unregistered path and mask the real error behind a
// generic 404. If sig/pem are non-nil, they are served too (as a real
// publisher's release would when HasCosign is set); when nil, cosign's
// fail-closed path is exercised because the files never exist locally.
func serveCore(t *testing.T, core *Core, assetName, altName string, archive, sig, pem []byte) *httptest.Server {
	t.Helper()
	checksum := sha256.Sum256(archive)
	checksumsContent := fmt.Sprintf("%s  %s\n%s  %s\n",
		hex.EncodeToString(checksum[:]), assetName,
		hex.EncodeToString(checksum[:]), altName)

	mux := http.NewServeMux()
	for _, name := range uniqueStrings(assetName, altName) {
		name := name
		mux.HandleFunc("/"+core.Repo+"/releases/latest/download/"+name, func(w http.ResponseWriter, r *http.Request) {
			w.Write(archive)
		})
		if sig != nil {
			mux.HandleFunc("/"+core.Repo+"/releases/latest/download/"+name+".sig", func(w http.ResponseWriter, r *http.Request) {
				w.Write(sig)
			})
		}
		if pem != nil {
			mux.HandleFunc("/"+core.Repo+"/releases/latest/download/"+name+".pem", func(w http.ResponseWriter, r *http.Request) {
				w.Write(pem)
			})
		}
	}
	mux.HandleFunc("/"+core.Repo+"/releases/latest/download/checksums.txt", func(w http.ResponseWriter, r *http.Request) {
		w.Write([]byte(checksumsContent))
	})
	server := httptest.NewServer(mux)
	t.Cleanup(server.Close)
	return server
}

func uniqueStrings(vals ...string) []string {
	seen := make(map[string]bool, len(vals))
	var out []string
	for _, v := range vals {
		if !seen[v] {
			seen[v] = true
			out = append(out, v)
		}
	}
	return out
}

// TestInstall_HasCosign_MissingSignature_FailsClosed proves that a core
// with HasCosign set aborts the install when the publisher's .sig/.pem
// assets are not published (or not reachable) — the regression this
// issue exists to fix: previously a missing signature was silently
// treated as "nothing to verify" and the install proceeded anyway.
func TestInstall_HasCosign_MissingSignature_FailsClosed(t *testing.T) {
	goos, goarch, err := Platform()
	if err != nil {
		t.Fatalf("Platform: %v", err)
	}

	core := cosignCore()
	binaryData := []byte("#!/bin/sh\necho example\n")
	archive := buildArchive(t, core.BinaryName, binaryData)
	assetName := core.AssetName(goos, goarch)
	altName := core.AssetNameAlt(goos, goarch)

	// No .sig/.pem served — the fail-closed case.
	server := serveCore(t, core, assetName, altName, archive, nil, nil)

	binDir := t.TempDir()
	inst := &Installer{BinDir: binDir, baseURL: server.URL}

	err = inst.Install(context.Background(), core)
	if err == nil {
		t.Fatal("Install for HasCosign core with no published signature: got nil error, want error (fail closed)")
	}
	if !strings.Contains(err.Error(), "cosign") {
		t.Errorf("error = %q, want it to mention cosign", err.Error())
	}
	if _, statErr := os.Stat(filepath.Join(binDir, core.BinaryName)); !os.IsNotExist(statErr) {
		t.Error("binary should not be installed when cosign verification fails closed")
	}
}

// TestInstall_HasCosign_AllowUnsigned_InstallsWithWarning proves the
// explicit --allow-unsigned escape hatch (Installer.AllowUnsigned) lets
// the same missing-signature install proceed, and that it prints a
// warning rather than failing silently.
func TestInstall_HasCosign_AllowUnsigned_InstallsWithWarning(t *testing.T) {
	goos, goarch, err := Platform()
	if err != nil {
		t.Fatalf("Platform: %v", err)
	}

	core := cosignCore()
	binaryData := []byte("#!/bin/sh\necho example\n")
	archive := buildArchive(t, core.BinaryName, binaryData)
	assetName := core.AssetName(goos, goarch)
	altName := core.AssetNameAlt(goos, goarch)

	server := serveCore(t, core, assetName, altName, archive, nil, nil)

	binDir := t.TempDir()
	var warn bytes.Buffer
	inst := &Installer{BinDir: binDir, baseURL: server.URL, AllowUnsigned: true, Warn: &warn}

	if err := inst.Install(context.Background(), core); err != nil {
		t.Fatalf("Install with AllowUnsigned=true: %v", err)
	}
	got, err := os.ReadFile(filepath.Join(binDir, core.BinaryName))
	if err != nil {
		t.Fatalf("read installed binary: %v", err)
	}
	if !bytes.Equal(got, binaryData) {
		t.Errorf("installed content = %q, want %q", got, binaryData)
	}
	if !strings.Contains(warn.String(), "WARNING") {
		t.Errorf("warn output = %q, want a WARNING that verification was skipped", warn.String())
	}
}

// TestInstall_PinnedChecksum_MismatchFailsIndependentlyOfChecksumsTxt
// proves a manifest-pinned SHA256 is checked even when the fetched
// checksums.txt (same origin as the archive) agrees with the archive —
// a corrupted/mismatched pin must still abort the install.
func TestInstall_PinnedChecksum_MismatchFailsIndependentlyOfChecksumsTxt(t *testing.T) {
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
	assetName := core.AssetName(goos, goarch)
	altName := core.AssetNameAlt(goos, goarch)
	// The manifest pin is deliberately wrong for both the versioned and
	// fallback asset names, even though checksums.txt (served by
	// serveCore) correctly matches the archive — Install retries with
	// AssetNameAlt on any failure, so both must be pinned wrong for this
	// test to prove the pin (not the retry finding an unpinned name) is
	// what's enforced.
	wrong := strings.Repeat("0", 64)
	core.SHA256 = map[string]string{assetName: wrong, altName: wrong}

	server := serveCore(t, core, assetName, altName, archive, nil, nil)
	binDir := t.TempDir()
	inst := &Installer{BinDir: binDir, baseURL: server.URL}

	err = inst.Install(context.Background(), core)
	if err == nil {
		t.Fatal("Install with wrong pinned checksum: got nil error, want error")
	}
	if !strings.Contains(err.Error(), "pinned checksum") {
		t.Errorf("error = %q, want it to mention the pinned checksum", err.Error())
	}
	if _, statErr := os.Stat(filepath.Join(binDir, core.BinaryName)); !os.IsNotExist(statErr) {
		t.Error("binary should not be installed after pinned checksum mismatch")
	}
}

// TestInstall_PinnedChecksum_UsedInsteadOfChecksumsTxt proves a correct
// manifest-pinned SHA256 lets install succeed even when checksums.txt
// is wrong/unreachable — the whole point of pinning is to not depend on
// that same-origin file.
func TestInstall_PinnedChecksum_UsedInsteadOfChecksumsTxt(t *testing.T) {
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
	assetName := core.AssetName(goos, goarch)
	sum := sha256.Sum256(archive)
	core.SHA256 = map[string]string{assetName: hex.EncodeToString(sum[:])}

	mux := http.NewServeMux()
	mux.HandleFunc("/"+core.Repo+"/releases/latest/download/"+assetName, func(w http.ResponseWriter, r *http.Request) {
		w.Write(archive)
	})
	mux.HandleFunc("/"+core.Repo+"/releases/latest/download/checksums.txt", func(w http.ResponseWriter, r *http.Request) {
		// Deliberately broken/unreachable — pinned verification must not need it.
		http.Error(w, "not found", http.StatusNotFound)
	})
	server := httptest.NewServer(mux)
	defer server.Close()

	binDir := t.TempDir()
	inst := &Installer{BinDir: binDir, baseURL: server.URL}

	if err := inst.Install(context.Background(), core); err != nil {
		t.Fatalf("Install with correct pinned checksum and broken checksums.txt: %v", err)
	}
}
