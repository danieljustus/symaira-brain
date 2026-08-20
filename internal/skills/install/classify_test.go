package install

import (
	"testing"
)

// synthetic digest helpers: a digest is any non-empty string; absence on a
// side means the path is simply not in that map.
const (
	dA = "digest-base"
	dL = "digest-left"
	dR = "digest-right"
)

func kindOf(drifts []FileDrift, path string) DriftKind {
	for _, d := range drifts {
		if d.Path == path {
			return d.Kind
		}
	}
	return ""
}

// TestClassifyTable pins all five rows of the three-way classification
// table over synthetic digest sets — no filesystem involved.
func TestClassifyTable(t *testing.T) {
	cases := []struct {
		name       string
		base, left string
		right      string
		want       DriftKind
	}{
		{"unchanged", dA, dA, dA, DriftUnchanged},
		{"library-changed push", dA, dL, dA, DriftLibraryChanged},
		{"harness-changed", dA, dA, dR, DriftHarnessChanged},
		{"converged", dA, dL, dL, DriftConverged},
		{"conflict", dA, dL, dR, DriftConflict},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := ClassifyFile(tc.base, tc.left, tc.right)
			if got != tc.want {
				t.Fatalf("ClassifyFile(%q,%q,%q) = %s, want %s", tc.base, tc.left, tc.right, got, tc.want)
			}
		})
	}
}

// TestClassifyDeletionCases covers the three deletion/absence cases and
// proves each is distinguishable from "never existed" (which is the
// unchanged row with every side absent).
func TestClassifyDeletionCases(t *testing.T) {
	t.Run("deleted at harness", func(t *testing.T) {
		// base has it, library still has it, installed copy lost it.
		drifts := ClassifyDrift(
			map[string]string{"SKILL.md": dA},
			map[string]string{"SKILL.md": dA},
			map[string]string{},
		)
		if got := kindOf(drifts, "SKILL.md"); got != DriftHarnessChanged {
			t.Fatalf("deleted at harness = %s, want harness-changed", got)
		}
	})
	t.Run("added at harness", func(t *testing.T) {
		drifts := ClassifyDrift(
			map[string]string{},
			map[string]string{},
			map[string]string{"notes.txt": dR},
		)
		if got := kindOf(drifts, "notes.txt"); got != DriftHarnessChanged {
			t.Fatalf("added at harness = %s, want harness-changed", got)
		}
	})
	t.Run("deleted in library", func(t *testing.T) {
		drifts := ClassifyDrift(
			map[string]string{"SKILL.md": dA},
			map[string]string{},
			map[string]string{"SKILL.md": dA},
		)
		if got := kindOf(drifts, "SKILL.md"); got != DriftLibraryChanged {
			t.Fatalf("deleted in library = %s, want library-changed", got)
		}
	})
	t.Run("never existed", func(t *testing.T) {
		drifts := ClassifyDrift(
			map[string]string{},
			map[string]string{},
			map[string]string{},
		)
		if len(drifts) != 0 {
			t.Fatalf("never-existed set classified %d paths, want none", len(drifts))
		}
		// And a path absent everywhere is unchanged, not harness-changed.
		if got := ClassifyFile("", "", ""); got != DriftUnchanged {
			t.Fatalf("absent everywhere = %s, want unchanged", got)
		}
	})
	t.Run("deleted at harness vs never existed", func(t *testing.T) {
		// The distinguishing signal: deleted-at-harness is a path the
		// base knows; never-existed is not. Same right-hand side, two
		// different verdicts.
		deleted := ClassifyDrift(
			map[string]string{"gone.txt": dA},
			map[string]string{"gone.txt": dA},
			map[string]string{},
		)
		never := ClassifyDrift(
			map[string]string{},
			map[string]string{},
			map[string]string{},
		)
		if kindOf(deleted, "gone.txt") != DriftHarnessChanged {
			t.Fatalf("base-known absent right = %s, want harness-changed", kindOf(deleted, "gone.txt"))
		}
		if len(never) != 0 {
			t.Fatalf("unknown absent path must not be classified, got %+v", never)
		}
	})
}

// TestClassifyDriftUnionAndSort proves the union of all three sets is
// classified exactly once per path, sorted, and that every row carries its
// three digests for reporting.
func TestClassifyDriftUnionAndSort(t *testing.T) {
	base := map[string]string{"a.txt": dA, "only-base.txt": dA}
	left := map[string]string{"a.txt": dL, "only-left.txt": dL, "shared.txt": dL}
	right := map[string]string{"a.txt": dR, "only-right.txt": dR, "shared.txt": dL}

	drifts := ClassifyDrift(base, left, right)
	if len(drifts) != 5 {
		t.Fatalf("expected 5 union paths, got %d: %+v", len(drifts), drifts)
	}
	for i := 1; i < len(drifts); i++ {
		if drifts[i-1].Path >= drifts[i].Path {
			t.Fatalf("drifts not sorted: %+v", drifts)
		}
	}
	// Conflict carries all three digests.
	for _, d := range drifts {
		if d.Path != "a.txt" {
			continue
		}
		if d.Kind != DriftConflict || d.Base != dA || d.Left != dL || d.Right != dR {
			t.Fatalf("a.txt row wrong: %+v", d)
		}
	}
	// only-left.txt: added in library while harness unchanged → push.
	if kindOf(drifts, "only-left.txt") != DriftLibraryChanged {
		t.Fatalf("only-left.txt = %s, want library-changed", kindOf(drifts, "only-left.txt"))
	}
	// only-right.txt: added at harness → harness-changed.
	if kindOf(drifts, "only-right.txt") != DriftHarnessChanged {
		t.Fatalf("only-right.txt = %s, want harness-changed", kindOf(drifts, "only-right.txt"))
	}
	// only-base.txt: deleted on both sides → unchanged? No: it exists in
	// the base and nowhere else, so both sides deleted it → the row is
	// "changed" on both comparisons but left == right (both absent) →
	// converged. The base advanced to "gone".
	if kindOf(drifts, "only-base.txt") != DriftConverged {
		t.Fatalf("only-base.txt = %s, want converged", kindOf(drifts, "only-base.txt"))
	}
	// shared.txt: both sides moved to the same digest → converged.
	if kindOf(drifts, "shared.txt") != DriftConverged {
		t.Fatalf("shared.txt = %s, want converged", kindOf(drifts, "shared.txt"))
	}
}

// TestConflictPolicyValues pins the policy vocabulary.
func TestConflictPolicyValues(t *testing.T) {
	for _, p := range []ConflictPolicy{ConflictAbort, ConflictPreferSource, ConflictPreferTarget, ConflictManual} {
		if !ValidConflictPolicy(p) {
			t.Fatalf("policy %s must be valid", p)
		}
	}
	if ValidConflictPolicy("explode") {
		t.Fatal("unknown policy must be invalid")
	}
	if DefaultConflictPolicy != ConflictAbort {
		t.Fatalf("default policy = %s, want abort", DefaultConflictPolicy)
	}
}

// TestClassifyEmptyDigestIsAbsence pins that an empty-string digest is
// treated as absence, so callers can pass maps built with "" for missing.
func TestClassifyEmptyDigestIsAbsence(t *testing.T) {
	// Deleted in library (right side still holds the base digest).
	if got := ClassifyFile(dA, "", dA); got != DriftLibraryChanged {
		t.Fatalf("empty left = %s, want library-changed (deleted in library)", got)
	}
	// Deleted on both sides: converged (base advances to "gone").
	if got := ClassifyFile(dA, "", ""); got != DriftConverged {
		t.Fatalf("empty left+right = %s, want converged", got)
	}
	// All-empty digests mean the file never existed.
	if got := ClassifyFile("", "", ""); got != DriftUnchanged {
		t.Fatalf("all-empty = %s, want unchanged", got)
	}
}

func TestSummarizeDriftSharesStatusAndPullOutcomes(t *testing.T) {
	outcome := SummarizeDrift([]FileDrift{
		{Path: "library.txt", Kind: DriftLibraryChanged},
		{Path: "harness.txt", Kind: DriftHarnessChanged},
		{Path: "converged.txt", Kind: DriftConverged},
		{Path: "conflict.txt", Kind: DriftConflict},
	})
	if outcome.Status != StatusConflict {
		t.Fatalf("status = %s, want conflict", outcome.Status)
	}
	for _, path := range []string{"harness.txt", "converged.txt"} {
		if !outcome.Pullable[path] {
			t.Errorf("%s should be pullable", path)
		}
	}
	if !outcome.Refusable["conflict.txt"] {
		t.Fatal("conflict.txt should be refusable")
	}
	if outcome.Pullable["library.txt"] {
		t.Error("library-only drift must not be pullable")
	}
}
