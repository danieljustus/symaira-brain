package install

import (
	"encoding/json"
	"os"
	"path/filepath"
	"sort"
)

// DriftKind classifies one file across the three digest sets: base (the
// frozen snapshot written at install time), left (a freshly rendered
// library output) and right (the installed copy). The classification is the
// three-way table of docs/two-way-sync.md §3:
//
//	base ↔ left | base ↔ right | result
//	=           | =            | unchanged
//	≠           | =            | library changed → push
//	=           | ≠            | harness changed
//	≠           | ≠, left=right| converged; advance base only
//	≠           | ≠, left≠right| conflict
//
// Presence is part of the digest: a file absent on a side has no digest
// there. Deletions therefore classify as ordinary drift (base-has /
// right-lacks = deleted at the harness; base-lacks / right-has = added at
// the harness; base-has / left-lacks = deleted in the library), and
// "never existed" is distinguishable because all three sides are absent
// (→ unchanged).
type DriftKind string

const (
	// DriftUnchanged reports that all three sides agree.
	DriftUnchanged DriftKind = "unchanged"
	// DriftLibraryChanged reports the library changed since install while
	// the installed copy still matches the base snapshot; the repair
	// direction is push (reinstall).
	DriftLibraryChanged DriftKind = "library-changed"
	// DriftHarnessChanged reports the installed copy diverged from the
	// base snapshot while the library did not; the repair direction is
	// pull (carry the edit back into the library).
	DriftHarnessChanged DriftKind = "harness-changed"
	// DriftConverged reports both sides changed but now agree with each
	// other; only the base snapshot needs to advance.
	DriftConverged DriftKind = "converged"
	// DriftConflict reports both sides changed to different content; no
	// automatic repair exists.
	DriftConflict DriftKind = "conflict"
)

// FileDrift is one file's classification with the digests that produced it
// ("" on sides where the file is absent).
type FileDrift struct {
	Path  string    `json:"path"`
	Kind  DriftKind `json:"kind"`
	Base  string    `json:"base,omitempty"`
	Left  string    `json:"left,omitempty"`
	Right string    `json:"right,omitempty"`
}

// ClassifyFile classifies a single file from its digest on each side. A
// digest of "" means the file does not exist on that side.
func ClassifyFile(base, left, right string) DriftKind {
	switch {
	case base == left && base == right:
		return DriftUnchanged
	case base != left && base == right:
		return DriftLibraryChanged
	case base == left && base != right:
		return DriftHarnessChanged
	case left == right:
		return DriftConverged
	default:
		return DriftConflict
	}
}

type DriftOutcome struct {
	Status    StatusKind
	Pullable  map[string]bool
	Refusable map[string]bool
}

// SummarizeDrift maps the shared three-way classifications to the outcome
// vocabulary used by both status and pull. Harness-side edits and converged
// files are safe pull candidates; conflicts are always refused.
func SummarizeDrift(drifts []FileDrift) DriftOutcome {
	out := DriftOutcome{
		Status:    StatusInSync,
		Pullable:  map[string]bool{},
		Refusable: map[string]bool{},
	}
	for _, drift := range drifts {
		switch drift.Kind {
		case DriftConflict:
			out.Status = StatusConflict
			out.Refusable[drift.Path] = true
		case DriftHarnessChanged:
			out.Pullable[drift.Path] = true
			if out.Status != StatusConflict {
				out.Status = StatusHarnessChanged
			}
		case DriftConverged:
			out.Pullable[drift.Path] = true
		case DriftLibraryChanged:
			if out.Status == StatusInSync {
				out.Status = StatusStale
			}
		}
	}
	return out
}

// ClassifyDrift classifies every path in the union of the three digest
// sets. base/left/right are keyed by slash-separated relative path; a path
// missing from a set is treated as absent (digest ""). The result is sorted
// by path. The function is pure: no filesystem access, fully testable
// against synthetic maps.
func ClassifyDrift(base, left, right map[string]string) []FileDrift {
	paths := map[string]bool{}
	for p := range base {
		paths[p] = true
	}
	for p := range left {
		paths[p] = true
	}
	for p := range right {
		paths[p] = true
	}
	out := make([]FileDrift, 0, len(paths))
	for p := range paths {
		out = append(out, FileDrift{
			Path:  p,
			Kind:  ClassifyFile(base[p], left[p], right[p]),
			Base:  base[p],
			Left:  left[p],
			Right: right[p],
		})
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Path < out[j].Path })
	return out
}

// ConflictPolicy is the caller-side parameter for what a run does when a
// file classifies as conflict. The classifier itself never picks a winner;
// the default is abort, which reports and writes nothing.
type ConflictPolicy string

const (
	// ConflictAbort (default) aborts the run with a report naming target,
	// skill and file; nothing is written.
	ConflictAbort ConflictPolicy = "abort"
	// ConflictPreferSource lets the library side win (reinstall).
	ConflictPreferSource ConflictPolicy = "prefer-source"
	// ConflictPreferTarget lets the harness side win (pull).
	ConflictPreferTarget ConflictPolicy = "prefer-target"
	// ConflictManual leaves conflicts to the user; the run skips them.
	ConflictManual ConflictPolicy = "manual"
)

// DefaultConflictPolicy is the policy used when none is given: abort with a
// report, no silent winner.
const DefaultConflictPolicy = ConflictAbort

// ValidConflictPolicy reports whether p is a known policy value.
func ValidConflictPolicy(p ConflictPolicy) bool {
	switch p {
	case ConflictAbort, ConflictPreferSource, ConflictPreferTarget, ConflictManual:
		return true
	}
	return false
}

// baseHashes loads the persisted base snapshot digests for a skill from its
// manifest.json, keyed by slash-separated relative path. The manifest
// itself is never part of the file set. A missing manifest returns
// ErrNotManaged so callers can distinguish "never managed" from a broken
// snapshot.
func baseHashes(baseDir string) (map[string]string, error) {
	data, err := os.ReadFile(filepath.Join(baseDir, manifestFile))
	if err != nil {
		if os.IsNotExist(err) {
			return nil, ErrNotManaged
		}
		return nil, err
	}
	var m BaseManifest
	if err := json.Unmarshal(data, &m); err != nil {
		return nil, err
	}
	out := make(map[string]string, len(m.Files))
	for path, e := range m.Files {
		out[path] = e.SHA256
	}
	return out, nil
}
