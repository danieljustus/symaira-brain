package install

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

func Diff(renderedPath, installedPath string, opts Options) ([]Change, error) {
	// #123: when the install target is a symlink into the render directory
	// (the default symlink mode), a two-way comparison is structurally
	// blind — both paths resolve to the same tree, so every digest matches
	// itself and drift is never reported. When a base snapshot (#124)
	// exists for this target/name, anchor the comparison on it instead:
	// harness-side edits appear in installed-vs-base and library drift in
	// rendered-vs-base. Without a base (never managed, or installed before
	// base snapshots existed) the legacy comparison runs.
	if opts.BaseDir != "" && opts.Target != "" && isSymlink(installedPath) {
		baseDir, err := BasePath(opts.Target, filepath.Base(renderedPath), opts)
		if err == nil {
			if _, statErr := os.Stat(filepath.Join(baseDir, manifestFile)); statErr == nil {
				return diffAgainstBase(renderedPath, installedPath, baseDir)
			}
		}
	}
	return diffTwoWay(renderedPath, installedPath)
}

// diffTwoWay is the legacy two-path comparison: the change list answers
// "what would change if the installed tree were replaced by the rendered
// tree" (added = only in rendered, modified = different content, removed =
// only in installed). The install marker is excluded from both sides.
func diffTwoWay(renderedPath, installedPath string) ([]Change, error) {
	left, err := fileHashes(renderedPath)
	if err != nil {
		return nil, err
	}
	right, err := fileHashes(installedPath)
	if err != nil {
		return nil, err
	}
	seen := map[string]bool{}
	changes := []Change{}
	for path, lhash := range left {
		seen[path] = true
		if rhash, ok := right[path]; !ok {
			changes = append(changes, Change{Path: path, Status: "added"})
		} else if rhash != lhash {
			changes = append(changes, Change{Path: path, Status: "modified", Diff: diffFileContent(filepath.ToSlash(path), filepath.Join(renderedPath, path), filepath.Join(installedPath, path))})
		}
	}
	for path := range right {
		if !seen[path] {
			changes = append(changes, Change{Path: path, Status: "removed"})
		}
	}
	sort.Slice(changes, func(i, j int) bool { return changes[i].Path < changes[j].Path })
	return changes, nil
}

// diffAgainstBase compares both the rendered tree and the installed tree
// against the persisted base snapshot. A file is modified when either side
// diverges from the base; added when it exists on a side but not in the
// base; removed when only one current side has lost a base file. A file
// deleted from both current sides is converged and omitted. The snapshot's
// own manifest.json is excluded from the base side.
func diffAgainstBase(renderedPath, installedPath, baseDir string) ([]Change, error) {
	rendered, err := fileHashes(renderedPath)
	if err != nil {
		return nil, err
	}
	installed, err := fileHashes(installedPath)
	if err != nil {
		return nil, err
	}
	base, err := fileHashes(baseDir)
	if err != nil {
		return nil, err
	}
	delete(base, manifestFile)

	status := map[string]string{}
	paths := map[string]bool{}
	for path := range rendered {
		paths[path] = true
	}
	for path := range installed {
		paths[path] = true
	}
	for path := range base {
		paths[path] = true
	}
	for path := range paths {
		if change, ok := changeStatusForDrift(ClassifyFile(base[path], rendered[path], installed[path]), base[path], rendered[path], installed[path]); ok {
			status[path] = change
		}
	}
	changes := []Change{}
	for path, st := range status {
		c := Change{Path: path, Status: st}
		if st == "modified" {
			// Left = freshly rendered staging copy, right = installed
			// copy. In symlink mode the two trees differ (staging render
			// vs render cache), so this shows the real drift the dialog
			// is asking about — harness-side edits and library changes
			// combined, exactly like the two-way comparison.
			c.Diff = diffFileContent(filepath.ToSlash(path), filepath.Join(renderedPath, path), filepath.Join(installedPath, path))
		}
		changes = append(changes, c)
	}
	sort.Slice(changes, func(i, j int) bool { return changes[i].Path < changes[j].Path })
	return changes, nil
}

func changeStatusForDrift(kind DriftKind, base, rendered, installed string) (string, bool) {
	if kind == DriftUnchanged {
		return "", false
	}
	// A baseline file deleted from both current trees is converged, not an
	// actionable removal. Other converged paths keep Diff's historical output
	// status so callers that use it to advance the baseline remain compatible.
	if kind == DriftConverged && base != "" && rendered == "" && installed == "" {
		return "", false
	}
	switch {
	case base == "":
		return "added", true
	case rendered == "" || installed == "":
		return "removed", true
	default:
		return "modified", true
	}
}

// diffLine is one line of an edit script: unchanged (' '), present only in
// the left/rendered side ('-') or only in the right/installed side ('+').
type diffLine struct {
	kind byte
	text string
}

const (
	// maxDiffCells bounds the LCS dynamic-programming table (product of the
	// two line counts) so pathological inputs cannot exhaust memory; files
	// beyond the bound are reported status-only.
	maxDiffCells = 1 << 22
	// maxDiffFileSize bounds how much of a file is read for a content diff;
	// larger files (multi-MiB resources) are reported status-only.
	maxDiffFileSize = 1 << 20
	// diffContext is the number of unchanged context lines around each
	// change in the unified output.
	diffContext = 3
)

// diffFileContent computes a unified-style content diff between leftPath
// (freshly rendered) and rightPath (installed copy). displayPath names the
// file in the diff headers. It is best-effort enrichment: an empty result
// means "no content diff available" (either side missing or binary, file
// too large, or an unreadable file) — the change list itself is never
// downgraded by a diff failure.
func diffFileContent(displayPath, leftPath, rightPath string) string {
	left, err := readDiffLines(leftPath)
	if err != nil {
		return ""
	}
	right, err := readDiffLines(rightPath)
	if err != nil {
		return ""
	}
	if left == nil || right == nil {
		return ""
	}
	if len(left)*len(right) > maxDiffCells {
		return ""
	}
	return unifiedLineDiff(displayPath, left, right)
}

// readDiffLines returns the file's content as lines (trailing newline
// stripped). It returns (nil, nil) for a missing or binary file — callers
// treat that as "no content diff". A present-but-empty file yields an empty
// (non-nil) slice so it can be diffed against a non-empty counterpart.
func readDiffLines(path string) ([]string, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return nil, nil
		}
		return nil, err
	}
	if len(data) > maxDiffFileSize || bytes.IndexByte(data, 0) >= 0 {
		return nil, nil
	}
	if len(data) == 0 {
		return []string{}, nil
	}
	// Strip one trailing newline so the split does not produce a phantom
	// empty final line.
	return strings.Split(strings.TrimSuffix(string(data), "\n"), "\n"), nil
}

// unifiedLineDiff renders left vs right as a unified diff with ---/+++
// file headers and @@ hunk headers, using an LCS-based edit script.
// displayPath names the file in the headers.
func unifiedLineDiff(displayPath string, left, right []string) string {
	return formatUnifiedDiff(displayPath, editScript(left, right))
}

// editScript computes an LCS-based edit script transforming left into
// right: ' ' unchanged, '-' only in left, '+' only in right, in
// left-to-right order.
func editScript(left, right []string) []diffLine {
	n, m := len(left), len(right)
	// lcs[i][j] = LCS length of left[:i] vs right[:j], packed row-major.
	table := make([]int32, (n+1)*(m+1))
	at := func(i, j int) int { return i*(m+1) + j }
	for i := 1; i <= n; i++ {
		for j := 1; j <= m; j++ {
			switch {
			case left[i-1] == right[j-1]:
				table[at(i, j)] = table[at(i-1, j-1)] + 1
			case table[at(i-1, j)] >= table[at(i, j-1)]:
				table[at(i, j)] = table[at(i-1, j)]
			default:
				table[at(i, j)] = table[at(i, j-1)]
			}
		}
	}
	script := make([]diffLine, 0, n+m)
	i, j := n, m
	for i > 0 && j > 0 {
		switch {
		case left[i-1] == right[j-1]:
			script = append(script, diffLine{kind: ' ', text: left[i-1]})
			i--
			j--
		case table[at(i-1, j)] >= table[at(i, j-1)]:
			script = append(script, diffLine{kind: '-', text: left[i-1]})
			i--
		default:
			script = append(script, diffLine{kind: '+', text: right[j-1]})
			j--
		}
	}
	for ; i > 0; i-- {
		script = append(script, diffLine{kind: '-', text: left[i-1]})
	}
	for ; j > 0; j-- {
		script = append(script, diffLine{kind: '+', text: right[j-1]})
	}
	for a, b := 0, len(script)-1; a < b; a, b = a+1, b-1 {
		script[a], script[b] = script[b], script[a]
	}
	return script
}

// formatUnifiedDiff renders an edit script in unified-diff form: per-file
// ---/+++ headers, @@ hunk headers with old/new line ranges, and lines
// prefixed with ' ', '-', or '+'. Hunks are merged when their changes come
// within diffContext lines of each other.
func formatUnifiedDiff(displayPath string, script []diffLine) string {
	var b strings.Builder
	fmt.Fprintf(&b, "--- a/%s\n+++ b/%s\n", displayPath, displayPath)
	changed := make([]int, 0, len(script))
	for i := range script {
		if script[i].kind != ' ' {
			changed = append(changed, i)
		}
	}
	for i := 0; i < len(changed); {
		start := changed[i] - diffContext
		if start < 0 {
			start = 0
		}
		end := changed[i] + diffContext + 1
		if end > len(script) {
			end = len(script)
		}
		i++
		for i < len(changed) && changed[i] < end {
			end = changed[i] + diffContext + 1
			if end > len(script) {
				end = len(script)
			}
			i++
		}
		var oldStart, newStart, oldCount, newCount int
		for p := start; p < end; p++ {
			switch script[p].kind {
			case ' ':
				if oldStart == 0 {
					oldStart = p + 1
				}
				oldCount++
				if newStart == 0 {
					newStart = p + 1
				}
				newCount++
			case '-':
				if oldStart == 0 {
					oldStart = p + 1
				}
				oldCount++
			case '+':
				if newStart == 0 {
					newStart = p + 1
				}
				newCount++
			}
		}
		fmt.Fprintf(&b, "@@ -%d,%d +%d,%d @@\n", oldStart, oldCount, newStart, newCount)
		for p := start; p < end; p++ {
			b.WriteByte(script[p].kind)
			b.WriteString(script[p].text)
			b.WriteByte('\n')
		}
	}
	return b.String()
}

// isSymlink reports whether path is a symbolic link (install symlink mode).
func isSymlink(path string) bool {
	fi, err := os.Lstat(path)
	return err == nil && fi.Mode()&os.ModeSymlink != 0
}

func fileHashes(root string) (map[string]string, error) {
	out := map[string]string{}
	if _, err := os.Stat(root); errors.Is(err, os.ErrNotExist) {
		return out, nil
	}
	// Resolve any symlink in the root path before walking, so WalkDir
	// sees the actual directory and Lstat-based DirEntry.IsDir() works
	// for symlink-mode installs where root is a symlink to a directory.
	if resolved, err := filepath.EvalSymlinks(root); err == nil {
		root = resolved
	}
	err := filepath.WalkDir(root, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.Type()&os.ModeSymlink != 0 {
			return nil
		}
		if d.IsDir() {
			return nil
		}
		rel, err := filepath.Rel(root, path)
		if err != nil {
			return err
		}
		if rel == markerFile {
			return nil
		}
		f, err := os.Open(path)
		if err != nil {
			return err
		}
		hasher := sha256.New()
		if _, err := io.Copy(hasher, f); err != nil {
			_ = f.Close()
			return err
		}
		if err := f.Close(); err != nil {
			return err
		}
		out[rel] = hex.EncodeToString(hasher.Sum(nil))
		return nil
	})
	return out, err
}
