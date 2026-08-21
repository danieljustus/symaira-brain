package audit

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
)

// ChainAnchor holds the external checkpoint for detecting audit-log
// truncation. Hash-chaining alone detects modification of retained entries
// but not deletion of the tail — an attacker with write access can delete
// the most recent entries, and a chain check over what remains passes
// because there is nothing anchoring "how many entries should exist."
//
// The ChainAnchor file is a separate, small file that records the hash of
// the most recent entry and the total entry count. It should be stored on a
// different volume or at least with stricter permissions than the log file
// itself. On Linux, the anchor file can additionally use chattr +a
// (append-only) to prevent deletion.
type ChainAnchor struct {
	// LastEntryHash is the SHA-256 of the most recent audit log entry.
	LastEntryHash string `json:"last_entry_hash"`
	// EntryCount is the total number of entries in the log.
	EntryCount int64 `json:"entry_count"`
	// SchemaVersion for forward compatibility.
	SchemaVersion int `json:"schema_version"`
}

// CurrentSchemaVersion is the version of the ChainAnchor schema.
const CurrentSchemaVersion = 1

// DefaultAnchorPath returns the default path for the chain anchor file
// relative to the audit log path.
func DefaultAnchorPath(logPath string) string {
	return logPath + ".anchor"
}

// WriteCheckpoint writes the current chain head to the anchor file.
// The anchor file is atomically replaced (write to temp, rename).
func WriteCheckpoint(anchorPath string, hash string, count int64) error {
	anchor := ChainAnchor{
		LastEntryHash: hash,
		EntryCount:    count,
		SchemaVersion: CurrentSchemaVersion,
	}

	data := fmt.Sprintf(`{"schema_version":%d,"last_entry_hash":"%s","entry_count":%d}`+"\n",
		anchor.SchemaVersion, anchor.LastEntryHash, anchor.EntryCount)

	dir := filepath.Dir(anchorPath)
	if err := os.MkdirAll(dir, 0700); err != nil {
		return fmt.Errorf("audit: mkdir anchor dir: %w", err)
	}

	// Atomic write: write to temp file, then rename.
	tmpPath := anchorPath + ".tmp"
	if err := os.WriteFile(tmpPath, []byte(data), 0600); err != nil {
		return fmt.Errorf("audit: write anchor tmp: %w", err)
	}
	if err := os.Rename(tmpPath, anchorPath); err != nil {
		return fmt.Errorf("audit: rename anchor: %w", err)
	}
	return nil
}

// ReadCheckpoint reads and parses the chain anchor file.
func ReadCheckpoint(anchorPath string) (*ChainAnchor, error) {
	data, err := os.ReadFile(anchorPath)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil // no checkpoint yet
		}
		return nil, fmt.Errorf("audit: read anchor: %w", err)
	}

	var anchor ChainAnchor
	if err := json.Unmarshal(data, &anchor); err != nil {
		return nil, fmt.Errorf("audit: parse anchor: %w", err)
	}

	return &anchor, nil
}

// HashEntry computes the SHA-256 hash of an audit event entry.
// The hash includes the entry data and the previous entry's hash for
// chain integrity (tamper evidence against modification).
func HashEntry(entryData string, prevHash string) string {
	h := sha256.Sum256([]byte(prevHash + entryData))
	return hex.EncodeToString(h[:])
}

// VerifyChain checks that a sequence of entries forms a valid hash chain
// starting from the given initial hash (typically "" for the first entry)
// and ending with the given expected final hash.
func VerifyChain(entries []string, initialHash string, expectedFinalHash string) bool {
	prev := initialHash
	for _, entry := range entries {
		computed := HashEntry(entry, prev)
		prev = computed
	}
	return prev == expectedFinalHash
}

// VerifyAnchor checks that the hash chain up to the given entries matches
// the checkpoint. Returns true if the chain is valid and untruncated.
func VerifyAnchor(entries []string, initialHash string, anchor *ChainAnchor) bool {
	if anchor == nil {
		return false
	}
	if int64(len(entries)) != anchor.EntryCount {
		return false
	}
	return VerifyChain(entries, initialHash, anchor.LastEntryHash)
}
