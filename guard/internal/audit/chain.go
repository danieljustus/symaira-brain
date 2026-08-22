package audit

import "github.com/danieljustus/symaira-corekit/auditkit"

// This file is a delegation shim over corekit/auditkit since 2026-08-22
// (repo-konsolidierung.md §9): the hash-chain and checkpoint primitives are
// shared infrastructure now. The public API of this package is unchanged.
//
// The chain here hashes raw entry lines; auditkit's Sink stores entries as
// chained envelopes and exposes the same primitives for line-level use.

// GenesisHash is the previous-hash value for the first entry in a chain.
const GenesisHash = auditkit.GenesisHash

// ChainAnchor holds the external checkpoint for detecting audit-log
// truncation. See auditkit.ChainAnchor.
type ChainAnchor = auditkit.ChainAnchor

// CurrentSchemaVersion is the version of the ChainAnchor schema.
const CurrentSchemaVersion = auditkit.AnchorSchemaVersion

// DefaultAnchorPath returns the default path for the chain anchor file
// relative to the audit log path.
func DefaultAnchorPath(logPath string) string {
	return auditkit.DefaultAnchorPath(logPath)
}

// WriteCheckpoint writes the current chain head to the anchor file
// (atomic replace). See auditkit.WriteCheckpoint.
func WriteCheckpoint(anchorPath string, hash string, count int64) error {
	return auditkit.WriteCheckpoint(anchorPath, hash, count)
}

// ReadCheckpoint reads and parses the chain anchor file. A missing anchor
// returns (nil, nil).
func ReadCheckpoint(anchorPath string) (*ChainAnchor, error) {
	return auditkit.ReadCheckpoint(anchorPath)
}

// HashEntry computes the SHA-256 hash of an audit event entry, mixing in the
// previous entry's hash. See auditkit.HashEntry.
func HashEntry(entryData string, prevHash string) string {
	return auditkit.HashEntry(entryData, prevHash)
}

// VerifyChain checks that entries form a valid hash chain. Empty lines are
// skipped. See auditkit.VerifyChain.
func VerifyChain(entries []string, initialHash string, expectedFinalHash string) bool {
	return auditkit.VerifyChain(entries, initialHash, expectedFinalHash)
}

// VerifyAnchor checks count + chain against the checkpoint. See
// auditkit.VerifyAnchor.
func VerifyAnchor(entries []string, initialHash string, anchor *ChainAnchor) bool {
	return auditkit.VerifyAnchor(entries, initialHash, anchor)
}
