package managed

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"time"
)

// ProvenanceSource identifies how a managed binary entered the managed
// directory.
type ProvenanceSource string

const (
	// SourceRelease marks binaries downloaded from a pinned GitHub release
	// and verified per the manifest (checksum, cosign where available).
	SourceRelease ProvenanceSource = "release"
	// SourceBrain marks binaries built locally from the in-repo module
	// sources (browse/, operate/, scope/) via `symbrain setup
	// --from-source` and installed through the managed installer.
	SourceBrain ProvenanceSource = "brain-source"
)

// Provenance records the origin of a managed binary. It is stored as a
// sidecar file <binary>.provenance.json next to the binary in the managed
// directory, so `doctor`, acceptance checks and humans can prove which
// binary is actually being used without trusting PATH order.
type Provenance struct {
	// Binary is the managed binary name (e.g. "symbrowse").
	Binary string `json:"binary"`
	// Source is release or brain-source.
	Source ProvenanceSource `json:"source"`
	// Version is the version reported by the installed binary
	// (`<binary> version --json`), or the manifest pin for release
	// installs.
	Version string `json:"version"`
	// Repo is the publishing repository ("owner/name") for release
	// installs; empty for brain-source installs.
	Repo string `json:"repo,omitempty"`
	// ReceiverCommit is the symaira-brain commit the module sources were
	// built from (brain-source installs only).
	ReceiverCommit string `json:"receiver_commit,omitempty"`
	// ModuleDir is the in-repo module directory ("browse", "operate",
	// "scope"; brain-source installs only).
	ModuleDir string `json:"module_dir,omitempty"`
	// Builder identifies the build toolchain, e.g. "go1.26.5" or the
	// first line of `swift --version` (brain-source installs only).
	Builder string `json:"builder,omitempty"`
	// BuiltAt is the install/build timestamp (RFC3339, UTC).
	BuiltAt time.Time `json:"built_at"`
	// BinarySHA256 is the SHA-256 of the installed binary payload.
	BinarySHA256 string `json:"binary_sha256"`
}

// ProvenancePath returns the sidecar path for a managed binary.
func ProvenancePath(binDir, binaryName string) string {
	return filepath.Join(binDir, binaryName+".provenance.json")
}

// WriteProvenance atomically writes the provenance sidecar for a binary.
func WriteProvenance(binDir string, prov *Provenance) error {
	data, err := json.MarshalIndent(prov, "", "  ")
	if err != nil {
		return fmt.Errorf("managed: marshal provenance: %w", err)
	}
	data = append(data, '\n')
	target := ProvenancePath(binDir, prov.Binary)
	tmp, err := os.CreateTemp(binDir, ".provenance-*")
	if err != nil {
		return fmt.Errorf("managed: create provenance temp: %w", err)
	}
	tmpPath := tmp.Name()
	if _, err := tmp.Write(data); err != nil {
		tmp.Close()
		os.Remove(tmpPath)
		return fmt.Errorf("managed: write provenance: %w", err)
	}
	if err := tmp.Close(); err != nil {
		os.Remove(tmpPath)
		return fmt.Errorf("managed: close provenance: %w", err)
	}
	if err := os.Chmod(tmpPath, 0o644); err != nil {
		os.Remove(tmpPath)
		return fmt.Errorf("managed: chmod provenance: %w", err)
	}
	if err := os.Rename(tmpPath, target); err != nil {
		os.Remove(tmpPath)
		return fmt.Errorf("managed: rename provenance: %w", err)
	}
	return nil
}

// ReadProvenance reads the provenance sidecar for a binary. It returns
// (nil, nil) when no sidecar exists — an absent sidecar means the binary
// predates provenance tracking or was placed manually, and is not an
// error. A malformed sidecar is an error: a corrupt origin record must
// never be silently treated as "unknown but fine".
func ReadProvenance(binDir, binaryName string) (*Provenance, error) {
	data, err := os.ReadFile(ProvenancePath(binDir, binaryName))
	if err != nil {
		if errors.Is(err, fs.ErrNotExist) {
			return nil, nil
		}
		return nil, fmt.Errorf("managed: read provenance: %w", err)
	}
	var prov Provenance
	if err := json.Unmarshal(data, &prov); err != nil {
		return nil, fmt.Errorf("managed: parse provenance for %s: %w", binaryName, err)
	}
	return &prov, nil
}

// HashBinary computes the SHA-256 of binary payload bytes.
func HashBinary(data []byte) string {
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}
