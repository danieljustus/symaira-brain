package render

import (
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

func openDestinationRoot(path string) (*os.Root, error) {
	return openDestinationRootSecure(path)
}

func writeRendered(bundle *skill.Bundle, dst string, item Rendered, target Target, treeHash string) error {
	parent := filepath.Dir(dst)
	root, err := openDestinationRoot(parent)
	if err != nil {
		return err
	}
	defer root.Close()
	return writeRenderedRoot(bundle, root, filepath.Base(dst), item, target, treeHash)
}

// outputManifestEntry is the bounded, deterministic description of one output
// tree entry. Directories are included so an extra empty directory cannot make
// a poisoned render look current.
type outputManifestEntry struct {
	Path   string `json:"path"`
	Kind   string `json:"kind"`
	Mode   string `json:"mode"`
	Size   int64  `json:"size"`
	SHA256 string `json:"sha256"`
}

const (
	maxOutputEntries = skill.MaxResourceEntries + 16
	maxOutputBytes   = skill.MaxTotalResourceBytes + skill.MaxInputSize
)

// renderFaultHook is test-only fault injection. It is deliberately a narrow
// seam around each filesystem operation so rollback tests can fail copying,
// writing, syncing, and swapping without changing production behavior.
var renderFaultHook func(operation, path string) error

func renderFault(operation, path string) error {
	if renderFaultHook == nil {
		return nil
	}
	return renderFaultHook(operation, path)
}

func writeRenderedRoot(bundle *skill.Bundle, dstRoot *os.Root, dstRel string, item Rendered, target Target, treeHash string) (result error) {
	clean, err := checkedOutputPath(dstRel)
	if err != nil {
		return err
	}
	parent := filepath.Dir(clean)
	if err := skill.EnsureRootPath(dstRoot, parent); err != nil {
		return err
	}
	unlock, err := lockDestination(dstRoot, parent, clean)
	if err != nil {
		return err
	}
	defer unlock()
	if info, statErr := dstRoot.Lstat(clean); statErr == nil {
		if info.Mode()&os.ModeSymlink != 0 {
			return fmt.Errorf("destination path %q is a symlink", dstRel)
		}
		if !info.IsDir() {
			return fmt.Errorf("destination path %q is not a directory", dstRel)
		}
	} else if !errors.Is(statErr, os.ErrNotExist) {
		return statErr
	}

	// Read the old marker before staging so install metadata survives an
	// atomic replacement, but never let marker contents decide freshness.
	existingMarker, markerErr := readMarkerRoot(dstRoot, clean)
	if markerErr != nil {
		return markerErr
	}
	stageRel, err := makeSiblingDir(dstRoot, parent, ".symskills-stage-")
	if err != nil {
		return err
	}
	stageLive := true
	defer func() {
		if stageLive {
			if cleanupErr := dstRoot.RemoveAll(stageRel); cleanupErr != nil {
				result = errors.Join(result, fmt.Errorf("cleanup staging tree: %w", cleanupErr))
			}
		}
	}()

	if err := materializeStage(bundle, dstRoot, stageRel, item); err != nil {
		return err
	}
	expected, err := collectOutputManifest(dstRoot, stageRel)
	if err != nil {
		return err
	}
	digest, err := manifestDigest(expected)
	if err != nil {
		return err
	}
	sh := sourceHashWithMetadata(treeHash, item.SkillMD, target, item.Files, item.MetadataFile, item.MetadataBytes)
	if markerMatches(existingMarker, sh, expected, digest) {
		actual, actualErr := collectOutputManifest(dstRoot, clean)
		if actualErr == nil && manifestsEqual(actual, expected) {
			return nil
		}
	}

	markerBytes, err := updatedMarker(existingMarker, sh, expected, digest)
	if err != nil {
		return err
	}
	if err := boundOutputWithMarker(expected, markerBytes); err != nil {
		return err
	}
	if err := writeMaterializedFile(dstRoot, filepath.ToSlash(filepath.Join(stageRel, ".symskills.json")), markerBytes, 0o644); err != nil {
		return err
	}
	if err := syncRootTree(dstRoot, stageRel); err != nil {
		return err
	}

	backupRel := ""
	oldExists := false
	if _, statErr := dstRoot.Lstat(clean); statErr == nil {
		oldExists = true
		backupRel, err = uniqueSiblingName(dstRoot, parent, ".symskills-backup-")
		if err != nil {
			return err
		}
		if err := renderFault("swap-backup", clean); err != nil {
			return err
		}
		if err := dstRoot.Rename(clean, backupRel); err != nil {
			return fmt.Errorf("move old rendered tree to backup: %w", err)
		}
		if err := syncRootDir(dstRoot, parent); err != nil {
			return rollbackSwap(dstRoot, stageRel, clean, backupRel, oldExists, fmt.Errorf("sync backup swap: %w", err), parent)
		}
	}
	if err := renderFault("swap-install", clean); err != nil {
		return rollbackSwap(dstRoot, stageRel, clean, backupRel, oldExists, err, parent)
	}
	if err := dstRoot.Rename(stageRel, clean); err != nil {
		return rollbackSwap(dstRoot, stageRel, clean, backupRel, oldExists, err, parent)
	}
	stageLive = false
	if err := syncRootDir(dstRoot, parent); err != nil {
		return rollbackSwap(dstRoot, clean, clean, backupRel, oldExists, err, parent)
	}
	if oldExists {
		if err := renderFault("swap-remove-backup", backupRel); err != nil {
			return rollbackSwap(dstRoot, clean, clean, backupRel, oldExists, err, parent)
		}
		if err := dstRoot.RemoveAll(backupRel); err != nil {
			return rollbackSwap(dstRoot, clean, clean, backupRel, oldExists, fmt.Errorf("cleanup backup tree: %w", err), parent)
		}
		if err := syncRootDir(dstRoot, parent); err != nil {
			return rollbackSwap(dstRoot, clean, clean, backupRel, oldExists, fmt.Errorf("sync backup cleanup: %w", err), parent)
		}
	}
	return nil
}

func checkedOutputPath(rel string) (string, error) {
	if rel == "" || filepath.IsAbs(rel) {
		return "", fmt.Errorf("destination path %q must be relative", rel)
	}
	clean := filepath.Clean(filepath.FromSlash(rel))
	if clean == "." || clean == ".." || strings.HasPrefix(clean, ".."+string(filepath.Separator)) {
		return "", fmt.Errorf("destination path %q escapes root", rel)
	}
	parts := strings.Split(clean, string(filepath.Separator))
	for _, part := range parts {
		if part == "" || len(part) > 255 || part == "." || part == ".." {
			return "", fmt.Errorf("destination path %q contains an unsafe component", rel)
		}
	}
	return clean, nil
}

func ensureRealDirectory(path string) error {
	abs, err := filepath.Abs(path)
	if err != nil {
		return err
	}
	volume := filepath.VolumeName(abs)
	rest := strings.TrimPrefix(abs, volume)
	current := volume + string(filepath.Separator)
	for _, part := range strings.Split(strings.TrimPrefix(rest, string(filepath.Separator)), string(filepath.Separator)) {
		if part == "" {
			continue
		}
		current = filepath.Join(current, part)
		info, statErr := os.Lstat(current)
		if errors.Is(statErr, os.ErrNotExist) {
			if err := os.Mkdir(current, 0o755); err != nil && !errors.Is(err, os.ErrExist) {
				return err
			}
			info, statErr = os.Lstat(current)
		}
		if statErr != nil {
			return statErr
		}
		if info.Mode()&os.ModeSymlink != 0 {
			// macOS exposes temporary directories through the stable /var and
			// /tmp aliases. They are system aliases, not caller-controlled
			// destination components; all descendants are still checked.
			if current != string(filepath.Separator)+"var" && current != string(filepath.Separator)+"tmp" {
				return fmt.Errorf("destination path %q contains symlink %q", path, current)
			}
		} else if !info.IsDir() {
			return fmt.Errorf("destination path %q contains non-directory %q", path, current)
		}
	}
	return nil
}

func uniqueSiblingName(root *os.Root, parent, prefix string) (string, error) {
	for attempt := 0; attempt < 128; attempt++ {
		var suffix [12]byte
		if _, err := rand.Read(suffix[:]); err != nil {
			return "", fmt.Errorf("generate temporary name: %w", err)
		}
		name := prefix + hex.EncodeToString(suffix[:])
		if len(name) > 255 {
			return "", errors.New("temporary name exceeds filesystem component limit")
		}
		rel := filepath.Join(parent, name)
		if _, err := root.Lstat(rel); errors.Is(err, os.ErrNotExist) {
			return rel, nil
		} else if err != nil {
			return "", err
		}
	}
	return "", errors.New("unable to allocate collision-free temporary name")
}

func makeSiblingDir(root *os.Root, parent, prefix string) (string, error) {
	for attempt := 0; attempt < 128; attempt++ {
		rel, err := uniqueSiblingName(root, parent, prefix)
		if err != nil {
			return "", err
		}
		if err := root.Mkdir(rel, 0o755); err == nil {
			return rel, nil
		} else if !errors.Is(err, os.ErrExist) {
			return "", err
		}
	}
	return "", errors.New("unable to create collision-free staging directory")
}

func rollbackSwap(root *os.Root, newRel, dstRel, backupRel string, oldExists bool, cause error, parent string) error {
	var rollbackErrs []error
	if newRel != "" {
		if _, err := root.Lstat(newRel); err == nil {
			if err := root.RemoveAll(newRel); err != nil {
				rollbackErrs = append(rollbackErrs, fmt.Errorf("remove new tree: %w", err))
			}
		}
	}
	if oldExists && backupRel != "" {
		if err := root.Rename(backupRel, dstRel); err != nil {
			rollbackErrs = append(rollbackErrs, fmt.Errorf("restore old tree: %w", err))
		} else if err := syncRootDir(root, parent); err != nil {
			rollbackErrs = append(rollbackErrs, fmt.Errorf("sync restored parent: %w", err))
		}
	}
	if len(rollbackErrs) > 0 {
		return errors.Join(append([]error{cause}, rollbackErrs...)...)
	}
	return cause
}
