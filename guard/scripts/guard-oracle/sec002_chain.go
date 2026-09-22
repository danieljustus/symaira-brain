package main

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"

	"github.com/danieljustus/symaira-brain/guard/internal/audit"
	// The guard shim does not re-export VerifyAnchorForLog; the
	// production function lives in corekit/auditkit (chain.go delegates
	// everything else), and the root module already requires it.
	"github.com/danieljustus/symaira-corekit/auditkit"
)

// SEC-002 cases: audit-chain verification bytes — HashEntry digests,
// VerifyChain/VerifyAnchor/VerifyAnchorForLog verdicts, and
// ReadCheckpoint anchor decoding (the Go chain.go delegation over
// corekit/auditkit).
//
// Determinism notes:
//   - All verdicts are pure functions of the input; no clock is involved.
//   - Expected hashes in inputs are computed with the production
//     audit.HashEntry, so cases chain real digests.
//   - Checkpoint READ uses a throwaway temp directory; parse diagnostics
//     are path-free, and only success/missing-file outcomes are pinned.
//     DEFERRED: malformed-anchor error bytes — encoding/json's wording
//     (e.g. "unexpected end of JSON input") has no serde_json
//     equivalent; pinning it would require a Go-json decode-error shim
//     in symbrain-guard-core (unported subsystem).

type hashEntryInput struct {
	Entry    string `json:"entry"`
	PrevHash string `json:"prev_hash"`
}

type verifyChainInput struct {
	Entries       []string `json:"entries"`
	InitialHash   string   `json:"initial_hash"`
	ExpectedFinal string   `json:"expected_final_hash"`
}

type verifyAnchorInput struct {
	Entries     []string           `json:"entries"`
	InitialHash string             `json:"initial_hash"`
	Anchor      *audit.ChainAnchor `json:"anchor"`
}

type verifyAnchorForLogInput struct {
	Entries     []string           `json:"entries"`
	InitialHash string             `json:"initial_hash"`
	Anchor      *audit.ChainAnchor `json:"anchor"`
	LogSize     int64              `json:"log_size"`
	ContentHash string             `json:"content_hash"`
}

type readCheckpointInput struct {
	Content *string `json:"content"` // nil: no anchor file exists
}

func sec002ChainCases() []oracleCase {
	var cases []oracleCase

	// Chain material: production digests over a mixed log (blank line
	// included so the skip rule is exercised everywhere).
	chainEntries := []string{`{"seq":1}`, ``, `{"seq":2}`, `{"seq":"<&>"}`}
	final := audit.GenesisHash
	for _, entry := range chainEntries {
		if entry == "" {
			continue
		}
		final = audit.HashEntry(entry, final)
	}
	nonGenesisInitial := "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
	finalFromNonGenesis := audit.GenesisHash
	for _, entry := range chainEntries {
		if entry == "" {
			continue
		}
		finalFromNonGenesis = audit.HashEntry(entry, finalFromNonGenesis)
	}

	for _, tc := range []struct{ id, entry, prev string }{
		{"audit_hash_entry_genesis_line", `{"seq":1}`, audit.GenesisHash},
		{"audit_hash_entry_chained_line", `{"seq":2}`, audit.HashEntry(`{"seq":1}`, audit.GenesisHash)},
		{"audit_hash_entry_non_genesis_prev", "payload", nonGenesisInitial},
		{"audit_hash_entry_empty_entry", "", final},
		{"audit_hash_entry_hash_of_html", `<script>&amp;</script>`, audit.GenesisHash},
	} {
		cases = append(cases, successCase(tc.id, "audit_hash_entry", hashEntryInput{Entry: tc.entry, PrevHash: tc.prev}, audit.HashEntry(tc.entry, tc.prev)))
	}

	modified := append([]string(nil), chainEntries...)
	modified[2] = `{"seq":2,"tampered":true}`
	truncated := chainEntries[:2]

	for _, tc := range []struct {
		id       string
		entries  []string
		initial  string
		expected string
	}{
		{"audit_chain_valid_multiline", chainEntries, audit.GenesisHash, final},
		{"audit_chain_modified_entry", modified, audit.GenesisHash, final},
		{"audit_chain_truncated_tail", truncated, audit.GenesisHash, final},
		{"audit_chain_blank_lines_only", []string{"", ""}, audit.GenesisHash, audit.GenesisHash},
		{"audit_chain_non_genesis_initial", chainEntries, nonGenesisInitial, finalFromNonGenesis},
		{"audit_chain_initial_mismatch", chainEntries, nonGenesisInitial, final},
	} {
		cases = append(cases, successCase(tc.id, "audit_verify_chain", verifyChainInput{
			Entries: tc.entries, InitialHash: tc.initial, ExpectedFinal: tc.expected,
		}, audit.VerifyChain(tc.entries, tc.initial, tc.expected)))
	}

	validAnchor := &audit.ChainAnchor{
		LastEntryHash: final,
		EntryCount:    int64(len(chainEntries)),
		SchemaVersion: audit.CurrentSchemaVersion,
	}
	legacyAnchor := &audit.ChainAnchor{
		LastEntryHash: final,
		EntryCount:    int64(len(chainEntries)),
		SchemaVersion: audit.CurrentSchemaVersion,
		LogSize:       1234,
		ContentHash:   "aa",
	}
	hashMismatch := &audit.ChainAnchor{
		LastEntryHash: nonGenesisInitial,
		EntryCount:    int64(len(chainEntries)),
		SchemaVersion: audit.CurrentSchemaVersion,
	}

	for _, tc := range []struct {
		id      string
		entries []string
		initial string
		anchor  *audit.ChainAnchor
	}{
		{"audit_anchor_valid", chainEntries, audit.GenesisHash, validAnchor},
		{"audit_anchor_nil", chainEntries, audit.GenesisHash, nil},
		{"audit_anchor_count_mismatch", truncated, audit.GenesisHash, validAnchor},
		{"audit_anchor_hash_mismatch", chainEntries, audit.GenesisHash, hashMismatch},
		{"audit_anchor_legacy_ignores_log_fields", chainEntries, audit.GenesisHash, legacyAnchor},
		{"audit_anchor_empty_log", []string{}, audit.GenesisHash, &audit.ChainAnchor{SchemaVersion: audit.CurrentSchemaVersion}},
	} {
		cases = append(cases, successCase(tc.id, "audit_verify_anchor", verifyAnchorInput{
			Entries: tc.entries, InitialHash: tc.initial, Anchor: tc.anchor,
		}, audit.VerifyAnchor(tc.entries, tc.initial, tc.anchor)))
	}

	logBytes := []byte(`{"seq":1}` + "\n" + `{"seq":2}` + "\n")
	logHash := sha256.Sum256(logBytes)
	contentHash := hex.EncodeToString(logHash[:])
	logEntries := []string{`{"seq":1}`, `{"seq":2}`}
	logAnchor := &audit.ChainAnchor{
		LastEntryHash: audit.HashEntry(`{"seq":2}`, audit.HashEntry(`{"seq":1}`, audit.GenesisHash)),
		EntryCount:    2,
		SchemaVersion: audit.CurrentSchemaVersion,
		LogSize:       int64(len(logBytes)),
		ContentHash:   contentHash,
	}
	for _, tc := range []struct {
		id          string
		entries     []string
		initial     string
		anchor      *audit.ChainAnchor
		logSize     int64
		contentHash string
	}{
		{"audit_anchor_log_valid", logEntries, audit.GenesisHash, logAnchor, int64(len(logBytes)), contentHash},
		{"audit_anchor_log_size_mismatch", logEntries, audit.GenesisHash, logAnchor, int64(len(logBytes)) + 1, contentHash},
		{"audit_anchor_log_content_mismatch", logEntries, audit.GenesisHash, logAnchor, int64(len(logBytes)), nonGenesisInitial},
		{"audit_anchor_log_legacy_without_hash", logEntries, audit.GenesisHash, validAnchor, int64(len(logBytes)), contentHash},
		{"audit_anchor_log_nil", logEntries, audit.GenesisHash, nil, int64(len(logBytes)), contentHash},
	} {
		cases = append(cases, successCase(tc.id, "audit_verify_anchor_for_log", verifyAnchorForLogInput{
			Entries: tc.entries, InitialHash: tc.initial, Anchor: tc.anchor,
			LogSize: tc.logSize, ContentHash: tc.contentHash,
		}, auditkit.VerifyAnchorForLog(tc.entries, tc.initial, tc.anchor, tc.logSize, tc.contentHash)))
	}

	fullAnchorJSON, err := json.Marshal(legacyAnchor)
	if err != nil {
		panic(err)
	}
	fullAnchorJSON = append(fullAnchorJSON, '\n')
	minimal := `{"last_entry_hash":"abc","entry_count":1,"schema_version":2}` + "\n"
	unknownFields := `{"last_entry_hash":"abc","entry_count":1,"schema_version":2,"future_flag":true}` + "\n"

	cases = append(cases, readCheckpointCase("audit_checkpoint_missing", nil))
	cases = append(cases, readCheckpointCase("audit_checkpoint_full", ptr(string(fullAnchorJSON))))
	cases = append(cases, readCheckpointCase("audit_checkpoint_minimal", ptr(minimal)))
	cases = append(cases, readCheckpointCase("audit_checkpoint_unknown_fields", ptr(unknownFields)))

	return cases
}

func ptr(value string) *string { return &value }

func readCheckpointCase(id string, content *string) oracleCase {
	input := readCheckpointInput{Content: content}
	dir, err := os.MkdirTemp("", "sec002-anchor-")
	if err != nil {
		panic(err)
	}
	defer func() {
		if rerr := os.RemoveAll(dir); rerr != nil {
			panic(rerr)
		}
	}()
	anchorPath := audit.DefaultAnchorPath(filepath.Join(dir, "audit.log"))
	if content != nil {
		if werr := os.WriteFile(anchorPath, []byte(*content), 0o600); werr != nil {
			panic(werr)
		}
	}
	anchor, rerr := audit.ReadCheckpoint(anchorPath)
	if rerr != nil {
		return errorCase(id, "audit_read_checkpoint", input, rerr)
	}
	return successCase(id, "audit_read_checkpoint", input, anchor)
}
