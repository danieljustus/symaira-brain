package render

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

func writeMaterializedFile(root *os.Root, path string, data []byte, mode os.FileMode) error {
	if err := renderFault("write", path); err != nil {
		return err
	}
	if err := skill.WriteRootFile(root, path, data, mode); err != nil {
		return err
	}
	return syncRootFile(root, path)
}

var errMarkerTooLarge = errors.New("render marker exceeds maximum input size")

func readMarkerRoot(root *os.Root, dstRel string) (map[string]interface{}, error) {
	path := filepath.ToSlash(filepath.Join(dstRel, ".symskills.json"))
	data, err := readMarkerBytesNoFollow(root, path)
	if errors.Is(err, errMarkerTooLarge) {
		return nil, err
	}
	if err != nil {
		return nil, nil
	}
	var marker map[string]interface{}
	if json.Unmarshal(data, &marker) != nil {
		return nil, nil
	}
	return marker, nil
}

func updatedMarker(existing map[string]interface{}, sourceHash string, manifest []outputManifestEntry, digest string) ([]byte, error) {
	if existing == nil {
		existing = map[string]interface{}{}
	}
	existing["source_hash"] = sourceHash
	existing["output_manifest"] = manifest
	existing["output_digest"] = digest
	data, err := json.MarshalIndent(existing, "", "  ")
	if err != nil {
		return nil, err
	}
	data = append(data, '\n')
	if int64(len(data)) > skill.MaxInputSize {
		return nil, errMarkerTooLarge
	}
	return data, nil
}

func markerMatches(marker map[string]interface{}, sourceHash string, expected []outputManifestEntry, digest string) bool {
	if marker == nil || marker["source_hash"] != sourceHash || marker["output_digest"] != digest {
		return false
	}
	data, ok := marker["output_manifest"]
	if !ok {
		return false
	}
	encoded, err := json.Marshal(data)
	if err != nil {
		return false
	}
	var actual []outputManifestEntry
	if json.Unmarshal(encoded, &actual) != nil {
		return false
	}
	return manifestsEqual(actual, expected)
}

func manifestsEqual(left, right []outputManifestEntry) bool {
	leftBytes, leftErr := json.Marshal(left)
	rightBytes, rightErr := json.Marshal(right)
	return leftErr == nil && rightErr == nil && string(leftBytes) == string(rightBytes)
}

func manifestDigest(manifest []outputManifestEntry) (string, error) {
	data, err := json.Marshal(manifest)
	if err != nil {
		return "", err
	}
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:]), nil
}

func boundOutputWithMarker(manifest []outputManifestEntry, marker []byte) error {
	var total int64
	for _, entry := range manifest {
		if entry.Kind == "file" {
			total += entry.Size
		}
	}
	total += int64(len(marker))
	if total > maxOutputBytes {
		return fmt.Errorf("rendered output including marker exceeds maximum size of %d bytes", maxOutputBytes)
	}
	return nil
}

func collectOutputManifest(root *os.Root, rel string) ([]outputManifestEntry, error) {
	entries := make([]outputManifestEntry, 0)
	var total int64
	var walk func(string, string) error
	walk = func(current, base string) error {
		dirRoot, err := openRenderDirNoFollow(root, current)
		if err != nil {
			return err
		}
		if dirRoot != root {
			defer dirRoot.Close()
		}
		dir, err := dirRoot.Open(".")
		if err != nil {
			return err
		}
		items, err := dir.ReadDir(-1)
		_ = dir.Close()
		if err != nil {
			return err
		}
		sort.Slice(items, func(i, j int) bool { return items[i].Name() < items[j].Name() })
		for _, item := range items {
			if current == rel && item.Name() == ".symskills.json" {
				continue
			}
			path := filepath.Join(current, item.Name())
			info, err := root.Lstat(path)
			if err != nil {
				return err
			}
			if len(entries) >= maxOutputEntries {
				return fmt.Errorf("rendered output exceeds maximum entry count of %d", maxOutputEntries)
			}
			relPath := filepath.ToSlash(strings.TrimPrefix(path, base+string(filepath.Separator)))
			if info.IsDir() {
				entries = append(entries, outputManifestEntry{Path: relPath, Kind: "dir", Mode: fmt.Sprintf("%04o", info.Mode().Perm())})
				if err := walk(path, base); err != nil {
					return err
				}
				continue
			}
			if info.Mode()&os.ModeSymlink != 0 || !info.Mode().IsRegular() {
				return fmt.Errorf("rendered output contains unsafe entry %q", relPath)
			}
			file, err := openRenderFileNoFollow(root, path)
			if err != nil {
				return err
			}
			data, readErr := io.ReadAll(file)
			closeErr := file.Close()
			if readErr != nil {
				return readErr
			}
			if closeErr != nil {
				return closeErr
			}
			total += int64(len(data))
			if total > maxOutputBytes {
				return fmt.Errorf("rendered output exceeds maximum size of %d bytes", maxOutputBytes)
			}
			sum := sha256.Sum256(data)
			entries = append(entries, outputManifestEntry{Path: relPath, Kind: "file", Mode: fmt.Sprintf("%04o", info.Mode().Perm()), Size: int64(len(data)), SHA256: hex.EncodeToString(sum[:])})
		}
		return nil
	}
	if err := walk(rel, filepath.Clean(rel)); err != nil {
		return nil, err
	}
	return entries, nil
}

func syncRootFile(root *os.Root, path string) error {
	if err := renderFault("sync-file", path); err != nil {
		return err
	}
	file, err := openRenderFileNoFollow(root, path)
	if err != nil {
		return err
	}
	defer file.Close()
	return file.Sync()
}

func syncRootDir(root *os.Root, path string) error {
	if err := renderFault("sync-dir", path); err != nil {
		return err
	}
	dirRoot, err := openRenderDirNoFollow(root, path)
	if err != nil {
		return err
	}
	if dirRoot != root {
		defer dirRoot.Close()
	}
	dir, err := dirRoot.Open(".")
	if err != nil {
		return err
	}
	defer dir.Close()
	return dir.Sync()
}

func syncRootTree(root *os.Root, rel string) error {
	info, err := root.Lstat(rel)
	if err != nil {
		return err
	}
	if !info.IsDir() {
		return syncRootFile(root, rel)
	}
	dirRoot, err := openRenderDirNoFollow(root, rel)
	if err != nil {
		return err
	}
	if dirRoot != root {
		defer dirRoot.Close()
	}
	dir, err := dirRoot.Open(".")
	if err != nil {
		return err
	}
	items, err := dir.ReadDir(-1)
	_ = dir.Close()
	if err != nil {
		return err
	}
	for _, item := range items {
		if err := syncRootTree(root, filepath.Join(rel, item.Name())); err != nil {
			return err
		}
	}
	return syncRootDir(root, rel)
}

func readDestinationDir(root *os.Root) ([]os.DirEntry, error) {
	dir, err := root.Open(".")
	if err != nil {
		return nil, err
	}
	defer dir.Close()
	return dir.ReadDir(-1)
}

// writeTargetMetadataRoot writes per-target metadata files into a rendered skill.
func writeCodexMetadataRoot(dstRoot *os.Root, dstRel string, item Rendered) error {
	content := fmt.Sprintf(`interface:
  display_name: %q
  short_description: %q
policy:
  allow_implicit_invocation: true
`, item.Name, item.Frontmatter.Description)
	path := filepath.ToSlash(filepath.Join(dstRel, "agents", "openai.yaml"))
	return writeMaterializedFile(dstRoot, path, []byte(content), 0o644)
}

// writeTargetMetadataRoot writes per-target metadata files into a rendered skill.
// Built-in targets with a fixed metadata contract (Codex) keep their
// generated file; user-defined targets use the bounded snapshot captured while
// rendering, so the materialized bytes cannot drift from the fingerprint.
func writeTargetMetadataRoot(dstRoot *os.Root, dstRel string, target Target, item Rendered) error {
	if target == TargetCodex {
		return writeCodexMetadataRoot(dstRoot, dstRel, item)
	}
	if item.MetadataFile == "" {
		return nil
	}
	path := filepath.ToSlash(filepath.Join(dstRel, filepath.FromSlash(item.MetadataFile)))
	return writeMaterializedFile(dstRoot, path, item.MetadataBytes, 0o644)
}

// ParseTarget converts a user-facing target string.
func ParseTarget(s string) (Target, error) {
	for _, spec := range Targets {
		if string(spec.Name) == s {
			return Target(s), nil
		}
	}
	valid := make([]string, 0, len(Targets)+1)
	valid = append(valid, "all")
	for _, spec := range Targets {
		valid = append(valid, string(spec.Name))
	}
	return "", fmt.Errorf("unknown target %q (valid: %s)", s, strings.Join(valid, ", "))
}
