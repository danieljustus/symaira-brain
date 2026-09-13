package managed

import (
	"bytes"
	"context"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"runtime"
	"testing"
)

func TestInstallLocal_InstallsBinaryAndProvenance(t *testing.T) {
	binDir := t.TempDir()
	inst := NewInstaller(binDir)
	payload := []byte("#!/bin/sh\necho local\n")
	prov := &Provenance{
		Source:         SourceBrain,
		ReceiverCommit: "0123456789abcdef0123456789abcdef01234567",
		ModuleDir:      "browse",
		Builder:        "go version go1.26.5 darwin/arm64",
	}
	if err := inst.InstallLocal("symbrowse", payload, prov); err != nil {
		t.Fatalf("InstallLocal: %v", err)
	}

	got, err := os.ReadFile(filepath.Join(binDir, "symbrowse"))
	if err != nil {
		t.Fatalf("read installed binary: %v", err)
	}
	if !bytes.Equal(got, payload) {
		t.Errorf("installed payload = %q, want %q", got, payload)
	}
	info, err := os.Stat(filepath.Join(binDir, "symbrowse"))
	if err != nil {
		t.Fatal(err)
	}
	if runtime.GOOS != "windows" && info.Mode().Perm()&0o111 == 0 {
		t.Error("installed binary is not executable")
	}

	stored, err := ReadProvenance(binDir, "symbrowse")
	if err != nil {
		t.Fatalf("ReadProvenance: %v", err)
	}
	if stored == nil {
		t.Fatal("no provenance sidecar written")
	}
	if stored.Source != SourceBrain {
		t.Errorf("provenance source = %q, want %q", stored.Source, SourceBrain)
	}
	if stored.BinarySHA256 != HashBinary(payload) {
		t.Errorf("provenance sha256 = %q, want %q", stored.BinarySHA256, HashBinary(payload))
	}
	if stored.BuiltAt.IsZero() {
		t.Error("provenance BuiltAt not stamped by InstallLocal")
	}
	if stored.Binary != "symbrowse" {
		t.Errorf("provenance binary = %q, want symbrowse", stored.Binary)
	}
}

func TestInstallLocal_RequiresProvenanceSource(t *testing.T) {
	inst := NewInstaller(t.TempDir())
	if err := inst.InstallLocal("symbrowse", []byte("x"), &Provenance{}); err == nil {
		t.Fatal("InstallLocal with empty provenance source: got nil, want error")
	}
	if err := inst.InstallLocal("symbrowse", []byte("x"), nil); err == nil {
		t.Fatal("InstallLocal with nil provenance: got nil, want error")
	}
}

func TestInstallLocal_ReplacesReleaseProvenance(t *testing.T) {
	binDir := t.TempDir()
	inst := NewInstaller(binDir)
	releaseProv := &Provenance{Source: SourceRelease, Version: "v0.8.0", Repo: "danieljustus/symaira-browse"}
	if err := inst.InstallLocal("symbrowse", []byte("old"), releaseProv); err != nil {
		t.Fatal(err)
	}
	brainProv := &Provenance{Source: SourceBrain, ReceiverCommit: "abc", ModuleDir: "browse"}
	if err := inst.InstallLocal("symbrowse", []byte("new"), brainProv); err != nil {
		t.Fatal(err)
	}
	stored, err := ReadProvenance(binDir, "symbrowse")
	if err != nil {
		t.Fatal(err)
	}
	if stored.Source != SourceBrain {
		t.Errorf("provenance after reinstall = %q, want brain-source", stored.Source)
	}
}

// TestInstall_WritesReleaseProvenance verifies the download path records
// release provenance so doctor/acceptance can prove binary origin, and
// that it replaces a stale brain-source sidecar.
func TestInstall_WritesReleaseProvenance(t *testing.T) {
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
	checksumHex := HashBinary(archive)
	assetName := core.AssetName(goos, goarch)
	checksumsContent := fmt.Sprintf("%s  %s\n", checksumHex, assetName)

	mux := http.NewServeMux()
	mux.HandleFunc("/"+core.Repo+"/releases/download/v1.0.0/"+assetName, func(w http.ResponseWriter, r *http.Request) {
		w.Write(archive)
	})
	mux.HandleFunc("/"+core.Repo+"/releases/download/v1.0.0/checksums.txt", func(w http.ResponseWriter, r *http.Request) {
		w.Write([]byte(checksumsContent))
	})
	server := httptest.NewServer(mux)
	defer server.Close()

	binDir := t.TempDir()
	// Pre-seed a stale brain-source sidecar; the release install must
	// replace it.
	inst := &Installer{BinDir: binDir, baseURL: server.URL}
	if err := inst.InstallLocal("example-core", []byte("old"), &Provenance{Source: SourceBrain, ModuleDir: "browse"}); err != nil {
		t.Fatal(err)
	}

	if err := inst.Install(context.Background(), core); err != nil {
		t.Fatalf("Install: %v", err)
	}
	stored, err := ReadProvenance(binDir, "example-core")
	if err != nil {
		t.Fatal(err)
	}
	if stored == nil || stored.Source != SourceRelease {
		t.Fatalf("provenance after release install = %+v, want source=release", stored)
	}
	if stored.Repo != core.Repo || stored.Version != core.Version {
		t.Errorf("provenance = %+v, want repo %q version %q", stored, core.Repo, core.Version)
	}
	if stored.BinarySHA256 != HashBinary(binaryData) {
		t.Errorf("release provenance sha256 mismatch")
	}
}
