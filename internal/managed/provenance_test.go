package managed

import (
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestProvenanceRoundtrip(t *testing.T) {
	binDir := t.TempDir()
	want := &Provenance{
		Binary:         "symbrowse",
		Source:         SourceBrain,
		Version:        "0.8.0",
		ReceiverCommit: "0123456789abcdef",
		ModuleDir:      "browse",
		Builder:        "go version go1.26.5 darwin/arm64",
		BuiltAt:        time.Date(2026, 9, 13, 12, 0, 0, 0, time.UTC),
		BinarySHA256:   "deadbeef",
	}
	if err := WriteProvenance(binDir, want); err != nil {
		t.Fatalf("WriteProvenance: %v", err)
	}
	got, err := ReadProvenance(binDir, "symbrowse")
	if err != nil {
		t.Fatalf("ReadProvenance: %v", err)
	}
	if got == nil {
		t.Fatal("ReadProvenance returned nil for written sidecar")
	}
	if *got != *want {
		t.Errorf("roundtrip mismatch:\n got %+v\nwant %+v", got, want)
	}
}

func TestReadProvenanceMissingReturnsNilNil(t *testing.T) {
	prov, err := ReadProvenance(t.TempDir(), "symbrowse")
	if err != nil {
		t.Fatalf("ReadProvenance on missing sidecar: %v, want nil error", err)
	}
	if prov != nil {
		t.Errorf("ReadProvenance on missing sidecar = %+v, want nil", prov)
	}
}

func TestReadProvenanceMalformedFailsClosed(t *testing.T) {
	binDir := t.TempDir()
	if err := os.WriteFile(ProvenancePath(binDir, "symbrowse"), []byte("{not json"), 0o644); err != nil {
		t.Fatal(err)
	}
	prov, err := ReadProvenance(binDir, "symbrowse")
	if err == nil {
		t.Fatalf("ReadProvenance on malformed sidecar: got %+v, want error", prov)
	}
}

func TestWriteProvenanceIsAtomic(t *testing.T) {
	binDir := t.TempDir()
	first := &Provenance{Binary: "symbrowse", Source: SourceBrain, Version: "0.1.0", BuiltAt: time.Now().UTC()}
	second := &Provenance{Binary: "symbrowse", Source: SourceRelease, Version: "0.2.0", Repo: "danieljustus/symaira-browse", BuiltAt: time.Now().UTC()}
	if err := WriteProvenance(binDir, first); err != nil {
		t.Fatal(err)
	}
	if err := WriteProvenance(binDir, second); err != nil {
		t.Fatal(err)
	}
	got, err := ReadProvenance(binDir, "symbrowse")
	if err != nil {
		t.Fatal(err)
	}
	if got.Source != SourceRelease || got.Version != "0.2.0" {
		t.Errorf("after overwrite: %+v, want release 0.2.0", got)
	}
	// No temp files must linger after a successful write.
	entries, err := os.ReadDir(binDir)
	if err != nil {
		t.Fatal(err)
	}
	for _, e := range entries {
		if filepath.Ext(e.Name()) != ".json" {
			t.Errorf("unexpected leftover file %s", e.Name())
		}
	}
}
