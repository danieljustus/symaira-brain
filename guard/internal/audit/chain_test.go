package audit

import (
	"os"
	"path/filepath"
	"testing"
)

func TestHashEntry(t *testing.T) {
	h1 := HashEntry("entry1", "")
	if h1 == "" {
		t.Fatal("HashEntry returned empty")
	}
	h2 := HashEntry("entry2", h1)
	if h2 == "" {
		t.Fatal("HashEntry returned empty for chained entry")
	}
	if h1 == h2 {
		t.Error("different entries should produce different hashes")
	}
}

func TestHashEntry_Deterministic(t *testing.T) {
	h1 := HashEntry("test", "")
	h2 := HashEntry("test", "")
	if h1 != h2 {
		t.Error("HashEntry should be deterministic")
	}
}

func TestVerifyChain(t *testing.T) {
	h1 := HashEntry("entry1", "")
	h2 := HashEntry("entry2", h1)

	if !VerifyChain([]string{"entry1", "entry2"}, "", h2) {
		t.Error("VerifyChain should pass for valid chain")
	}
}

func TestVerifyChain_ModifiedEntry(t *testing.T) {
	h1 := HashEntry("entry1", "")
	h2 := HashEntry("entry2", h1)

	// Modify entry1's content — chain should break
	modified1 := HashEntry("MODIFIED", "")
	modified2 := HashEntry("entry2", modified1)

	if VerifyChain([]string{"MODIFIED", "entry2"}, "", h2) {
		t.Error("VerifyChain should fail when an entry is modified")
	}

	if h2 == modified2 {
		t.Error("modified chain should have different final hash")
	}
}

func TestVerifyChain_TruncationDetection(t *testing.T) {
	// Build a 3-entry chain, then truncate to 2
	h1 := HashEntry("entry1", "")
	h2 := HashEntry("entry2", h1)
	h3 := HashEntry("entry3", h2)

	// The full chain verifies
	if !VerifyChain([]string{"entry1", "entry2", "entry3"}, "", h3) {
		t.Error("full chain should verify")
	}

	// Truncated chain (only entry1, entry2) ending at h2
	if VerifyChain([]string{"entry1", "entry2"}, "", h3) {
		t.Error("truncated chain should not match the full-chain final hash")
	}

	// Anchor-based detection
	anchor := &ChainAnchor{LastEntryHash: h3, EntryCount: 3}
	if VerifyAnchor([]string{"entry1", "entry2"}, "", anchor) {
		t.Error("VerifyAnchor should detect truncation via entry count mismatch")
	}
}

func TestVerifyAnchor(t *testing.T) {
	entries := []string{"a", "b", "c"}
	var prev string
	for _, e := range entries {
		prev = HashEntry(e, prev)
	}

	anchor := &ChainAnchor{LastEntryHash: prev, EntryCount: 3}
	if !VerifyAnchor(entries, "", anchor) {
		t.Error("VerifyAnchor should pass for valid chain and matching count")
	}
}

func TestVerifyAnchor_NilAnchor(t *testing.T) {
	if VerifyAnchor([]string{"a"}, "", nil) {
		t.Error("VerifyAnchor should fail with nil anchor")
	}
}

func TestWriteReadCheckpoint(t *testing.T) {
	dir := t.TempDir()
	anchorPath := filepath.Join(dir, "audit.log.anchor")

	err := WriteCheckpoint(anchorPath, "abc123", 42)
	if err != nil {
		t.Fatalf("WriteCheckpoint() error = %v", err)
	}

	anchor, err := ReadCheckpoint(anchorPath)
	if err != nil {
		t.Fatalf("ReadCheckpoint() error = %v", err)
	}
	if anchor == nil {
		t.Fatal("ReadCheckpoint returned nil")
	}
	if anchor.LastEntryHash != "abc123" {
		t.Errorf("last_entry_hash = %q, want abc123", anchor.LastEntryHash)
	}
	if anchor.EntryCount != 42 {
		t.Errorf("entry_count = %d, want 42", anchor.EntryCount)
	}
}

func TestReadCheckpoint_NotExists(t *testing.T) {
	anchor, err := ReadCheckpoint("/nonexistent/anchor")
	if err != nil {
		t.Fatalf("ReadCheckpoint() error = %v", err)
	}
	if anchor != nil {
		t.Error("expected nil for nonexistent anchor")
	}
}

func TestWriteCheckpoint_CreatesDir(t *testing.T) {
	dir := t.TempDir()
	deepPath := filepath.Join(dir, "sub", "dir", "audit.log.anchor")

	err := WriteCheckpoint(deepPath, "hash", 1)
	if err != nil {
		t.Fatalf("WriteCheckpoint() error = %v", err)
	}

	if _, err := os.Stat(deepPath); os.IsNotExist(err) {
		t.Error("anchor file was not created")
	}
}

func TestDefaultAnchorPath(t *testing.T) {
	path := DefaultAnchorPath("/var/log/symguard/audit.log")
	if path != "/var/log/symguard/audit.log.anchor" {
		t.Errorf("DefaultAnchorPath = %q, want /var/log/symguard/audit.log.anchor", path)
	}
}
