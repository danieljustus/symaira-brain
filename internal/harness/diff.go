package harness

import (
	"fmt"
	"strings"
)

// diffContext is the number of unchanged lines shown around each change in
// UnifiedDiff's output, matching the conventional `diff -u` default.
const diffContext = 3

// UnifiedDiff renders a unified diff of the change from old to new content,
// for `install --dry-run` / `uninstall --dry-run` output. It is a
// self-contained, line-based implementation (longest-common-subsequence)
// good enough for the small config files symbrain edits — not a
// general-purpose diff engine.
//
// path is used for both the "---" and "+++" headers; old may be nil to
// represent a file that does not exist yet (every line renders as added).
func UnifiedDiff(path string, old, new []byte) string {
	oldLines := splitLines(old)
	newLines := splitLines(new)

	ops := diffLines(oldLines, newLines)
	hunks := groupHunks(ops)
	if len(hunks) == 0 {
		return ""
	}

	var b strings.Builder
	fmt.Fprintf(&b, "--- %s\n", path)
	fmt.Fprintf(&b, "+++ %s\n", path)
	for _, h := range hunks {
		writeHunk(&b, h)
	}
	return b.String()
}

// splitLines splits content into lines without their trailing newline. A
// nil/empty input produces no lines.
func splitLines(content []byte) []string {
	if len(content) == 0 {
		return nil
	}
	s := strings.TrimSuffix(string(content), "\n")
	return strings.Split(s, "\n")
}

type opKind int

const (
	opEqual opKind = iota
	opDelete
	opInsert
)

type op struct {
	kind opKind
	line string
}

// diffLines computes an LCS-based edit script turning oldLines into
// newLines.
func diffLines(oldLines, newLines []string) []op {
	n, m := len(oldLines), len(newLines)

	// lcs[i][j] = length of the LCS of oldLines[i:] and newLines[j:].
	lcs := make([][]int, n+1)
	for i := range lcs {
		lcs[i] = make([]int, m+1)
	}
	for i := n - 1; i >= 0; i-- {
		for j := m - 1; j >= 0; j-- {
			switch {
			case oldLines[i] == newLines[j]:
				lcs[i][j] = lcs[i+1][j+1] + 1
			case lcs[i+1][j] >= lcs[i][j+1]:
				lcs[i][j] = lcs[i+1][j]
			default:
				lcs[i][j] = lcs[i][j+1]
			}
		}
	}

	ops := make([]op, 0, n+m)
	i, j := 0, 0
	for i < n && j < m {
		switch {
		case oldLines[i] == newLines[j]:
			ops = append(ops, op{kind: opEqual, line: oldLines[i]})
			i++
			j++
		case lcs[i+1][j] >= lcs[i][j+1]:
			ops = append(ops, op{kind: opDelete, line: oldLines[i]})
			i++
		default:
			ops = append(ops, op{kind: opInsert, line: newLines[j]})
			j++
		}
	}
	for ; i < n; i++ {
		ops = append(ops, op{kind: opDelete, line: oldLines[i]})
	}
	for ; j < m; j++ {
		ops = append(ops, op{kind: opInsert, line: newLines[j]})
	}
	return ops
}

type hunk struct {
	oldStart, oldCount int
	newStart, newCount int
	ops                []op
}

// groupHunks collapses the edit script into unified-diff hunks: each run of
// changes (insert/delete) keeps up to diffContext lines of unchanged
// context on either side, and two changes separated by more than
// 2*diffContext unchanged lines become separate hunks.
func groupHunks(ops []op) []hunk {
	// oldPos[k]/newPos[k]: 0-based line position in old/new that ops[k]
	// corresponds to (for an opInsert, oldPos is where it would fall
	// between old lines; symmetric for opDelete/newPos).
	oldPos := make([]int, len(ops)+1)
	newPos := make([]int, len(ops)+1)
	for k, o := range ops {
		oldPos[k+1] = oldPos[k]
		newPos[k+1] = newPos[k]
		switch o.kind {
		case opEqual:
			oldPos[k+1]++
			newPos[k+1]++
		case opDelete:
			oldPos[k+1]++
		case opInsert:
			newPos[k+1]++
		}
	}

	var changeIdx []int
	for k, o := range ops {
		if o.kind != opEqual {
			changeIdx = append(changeIdx, k)
		}
	}
	if len(changeIdx) == 0 {
		return nil
	}

	var hunks []hunk
	start := 0
	for start < len(changeIdx) {
		end := start
		for end+1 < len(changeIdx) {
			gap := changeIdx[end+1] - changeIdx[end] - 1 // unchanged ops between them
			if gap > 2*diffContext {
				break
			}
			end++
		}

		lo := changeIdx[start] - diffContext
		if lo < 0 {
			lo = 0
		}
		hi := changeIdx[end] + diffContext
		if hi > len(ops)-1 {
			hi = len(ops) - 1
		}

		hunks = append(hunks, buildHunk(ops[lo:hi+1], oldPos[lo], newPos[lo]))
		start = end + 1
	}
	return hunks
}

// buildHunk computes 1-based start lines and counts for a slice of ops
// beginning at old/new 0-based positions oldFrom/newFrom.
func buildHunk(ops []op, oldFrom, newFrom int) hunk {
	h := hunk{oldStart: oldFrom + 1, newStart: newFrom + 1, ops: ops}
	for _, o := range ops {
		switch o.kind {
		case opEqual:
			h.oldCount++
			h.newCount++
		case opDelete:
			h.oldCount++
		case opInsert:
			h.newCount++
		}
	}
	return h
}

func writeHunk(b *strings.Builder, h hunk) {
	fmt.Fprintf(b, "@@ -%d,%d +%d,%d @@\n", h.oldStart, h.oldCount, h.newStart, h.newCount)
	for _, o := range h.ops {
		switch o.kind {
		case opEqual:
			fmt.Fprintf(b, " %s\n", o.line)
		case opDelete:
			fmt.Fprintf(b, "-%s\n", o.line)
		case opInsert:
			fmt.Fprintf(b, "+%s\n", o.line)
		}
	}
}
