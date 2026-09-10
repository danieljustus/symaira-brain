// Package skill loads, validates, and imports portable Agent Skill bundles.
package skill

import (
	"bytes"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"unicode/utf8"

	"github.com/BurntSushi/toml"
	"github.com/danieljustus/symaira-brain/internal/skills/variant"
)

// LoadBundle reads SKILL.md and optional symskills.toml from a skill root.
func LoadBundle(root string) (*Bundle, error) {
	abs, err := filepath.Abs(root)
	if err != nil {
		return nil, err
	}
	rootCap, err := openBundleRoot(abs)
	if err != nil {
		return nil, err
	}

	raw, err := readRootFile(rootCap, "SKILL.md", "SKILL.md", MaxInputSize)
	if err != nil {
		return nil, err
	}
	fm, body, err := parseSkillMD(raw)
	if err != nil {
		return nil, err
	}

	manifest := Manifest{Targets: map[string]TargetConfig{}}
	if _, statErr := rootCap.Stat("symskills.toml"); statErr == nil {
		manifestRaw, readErr := readRootFile(rootCap, "symskills.toml", "symskills.toml", MaxInputSize)
		if readErr != nil {
			return nil, readErr
		}
		meta, decodeErr := toml.Decode(string(manifestRaw), &manifest)
		if decodeErr != nil {
			return nil, fmt.Errorf("parse symskills.toml: %w", decodeErr)
		}
		if err := validateManifestTypes(meta, manifest); err != nil {
			return nil, err
		}
	} else if !errors.Is(statErr, os.ErrNotExist) {
		return nil, fmt.Errorf("stat symskills.toml: %w", statErr)
	}
	if manifest.Targets == nil {
		manifest.Targets = map[string]TargetConfig{}
	}
	if manifest.Skill.Name == "" {
		manifest.Skill.Name = fm.Name
	}
	if manifest.Skill.Version == "" {
		manifest.Skill.Version = fm.Version
	}

	resources, err := loadResources(rootCap, abs)
	if err != nil {
		return nil, fmt.Errorf("inventory bundle resources: %w", err)
	}
	markdown, err := loadMarkdown(rootCap, resources)
	if err != nil {
		return nil, fmt.Errorf("read bundle markdown: %w", err)
	}
	overrides, err := loadBlockOverrides(rootCap, abs)
	if err != nil {
		return nil, fmt.Errorf("read overlay blocks: %w", err)
	}
	normalized := strings.ReplaceAll(string(raw), "\r\n", "\n")
	bodyOffset := strings.Count(normalized[:len(normalized)-len(body)], "\n")
	return &Bundle{
		Root:           abs,
		rootCap:        rootCap,
		Frontmatter:    fm,
		Manifest:       manifest,
		Body:           body,
		Resources:      resources,
		Markdown:       markdown,
		BlockOverrides: overrides,
		BodyLineOffset: bodyOffset,
	}, nil
}

func validateManifestTypes(meta toml.MetaData, manifest Manifest) error {
	for _, root := range []string{"skill", "targets", "terms"} {
		if !meta.IsDefined(root) {
			continue
		}
		typ := meta.Type(root)
		if typ != "Hash" && !(typ == "" && hasManifestChild(meta, root)) {
			return fmt.Errorf("expected a table for %s", root)
		}
	}
	for _, key := range meta.Keys() {
		if len(key) != 2 || (key[0] != "targets" && key[0] != "terms") {
			continue
		}
		if meta.Type(key...) != "Hash" {
			return fmt.Errorf("expected a table for %s.%s", key[0], key[1])
		}
	}
	_ = manifest
	return nil
}

func hasManifestChild(meta toml.MetaData, root string) bool {
	for _, key := range meta.Keys() {
		if len(key) > 1 && key[0] == root {
			return true
		}
	}
	return false
}

// IsMarkdown reports whether a bundle-relative path is a markdown resource
// that participates in block and term resolution. Only markdown is resolved:
// scripts and data files travel byte-identical, so a placeholder-looking
// string in a script is never rewritten.
func IsMarkdown(rel string) bool {
	lower := strings.ToLower(rel)
	return strings.HasSuffix(lower, ".md") || strings.HasSuffix(lower, ".markdown")
}

// isOverlayPath reports whether a bundle-relative path lives under overlays/,
// which is render input rather than shipped content.
func isOverlayPath(rel string) bool {
	return rel == "overlays" || strings.HasPrefix(rel, "overlays/")
}

// loadMarkdown reads every markdown resource outside overlays/ into memory.
func loadMarkdown(root *os.Root, resources []Resource) (map[string]string, error) {
	out := map[string]string{}
	for _, res := range resources {
		if isOverlayPath(res.Path) || !IsMarkdown(res.Path) {
			continue
		}
		data, err := readRootFile(root, res.Path, res.Path, MaxInputSize)
		if err != nil {
			return nil, fmt.Errorf("%s: %w", res.Path, err)
		}
		// Binary markdown remains byte-preserving unless it is sent through the
		// variant parser. Variant-bearing markdown is textual and must decode
		// strictly rather than producing replacement characters.
		if (bytes.Contains(data, []byte("{{term:")) || bytes.Contains(data, []byte("symskills:"))) && !utf8.Valid(data) {
			return nil, fmt.Errorf("invalid_utf8_variant_markdown: %s", res.Path)
		}
		out[res.Path] = string(data)
	}
	return out, nil
}

// loadBlockOverrides reads overlays/<dir>/blocks/<id>.md for every overlay
// directory. A missing overlays/ tree yields an empty map, not an error.
func loadBlockOverrides(root *os.Root, _anchor string) (map[string]map[string]string, error) {
	entries, err := readRootDir(root, "overlays")
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return map[string]map[string]string{}, nil
		}
		return nil, err
	}
	slices.SortFunc(entries, func(a, b os.DirEntry) int { return strings.Compare(a.Name(), b.Name()) })
	out := map[string]map[string]string{}
	for _, entry := range entries {
		if !entry.IsDir() {
			continue
		}
		targetName := entry.Name()
		blocksRel := filepath.ToSlash(filepath.Join("overlays", targetName, variant.BlocksDir))
		files, err := readRootDir(root, blocksRel)
		if err != nil {
			if errors.Is(err, os.ErrNotExist) {
				continue
			}
			return nil, err
		}
		slices.SortFunc(files, func(a, b os.DirEntry) int { return strings.Compare(a.Name(), b.Name()) })
		blocks := map[string]string{}
		for _, file := range files {
			if !IsMarkdown(file.Name()) || file.IsDir() {
				continue
			}
			rel := filepath.ToSlash(filepath.Join(blocksRel, file.Name()))
			info, err := root.Stat(filepath.FromSlash(rel))
			if err != nil {
				return nil, fmt.Errorf("%s: %w", file.Name(), err)
			}
			if !info.Mode().IsRegular() {
				continue
			}
			data, err := readRootFile(root, rel, rel, MaxInputSize)
			if err != nil {
				return nil, fmt.Errorf("%s: %w", file.Name(), err)
			}
			id := strings.TrimSuffix(strings.TrimSuffix(file.Name(), ".markdown"), ".md")
			if !utf8.Valid(data) {
				return nil, fmt.Errorf("invalid_utf8_overlay: %s", rel)
			}
			blocks[id] = string(data)
		}
		if len(blocks) > 0 {
			out[targetName] = blocks
		}
	}
	return out, nil
}

// loadResources inventories every non-SKILL.md file under root: relative
// path, size, permission mode, and executable flag. WalkDir order is
// lexical, so the result is deterministic. .git directories are skipped to
// match ImportSkill's copy semantics. Symlinked directories inside the bundle
// are traversed under their logical link path; targets outside the bundle are
// rejected rather than read.
func loadResources(root *os.Root, _anchor string) ([]Resource, error) {
	resources := []Resource{}
	entriesSeen := 0
	var totalBytes int64
	pending := []struct {
		rel   string
		depth int
	}{{rel: ".", depth: 0}}
	for len(pending) > 0 {
		current := pending[len(pending)-1]
		pending = pending[:len(pending)-1]
		if current.depth > MaxResourceDepth {
			return nil, fmt.Errorf("resource tree exceeds maximum depth of %d", MaxResourceDepth)
		}
		entries, err := readRootDir(root, current.rel)
		if err != nil {
			return nil, err
		}
		slices.SortFunc(entries, func(a, b os.DirEntry) int { return strings.Compare(a.Name(), b.Name()) })
		for i := len(entries) - 1; i >= 0; i-- {
			entry := entries[i]
			entriesSeen++
			if entriesSeen > MaxResourceEntries {
				return nil, fmt.Errorf("resource tree exceeds maximum entry count of %d", MaxResourceEntries)
			}
			rel := entry.Name()
			if current.rel != "." {
				rel = filepath.Join(current.rel, entry.Name())
			}
			if rel == "SKILL.md" {
				continue
			}
			info, statErr := root.Stat(filepath.FromSlash(rel))
			if statErr != nil {
				if entry.Type()&os.ModeSymlink != 0 {
					return nil, fmt.Errorf("resource %q escapes skill root: %w", filepath.ToSlash(rel), statErr)
				}
				return nil, statErr
			}
			if info.IsDir() {
				if entry.Name() != ".git" {
					pending = append(pending, struct {
						rel   string
						depth int
					}{rel: filepath.ToSlash(rel), depth: current.depth + 1})
				}
				continue
			}
			if !info.Mode().IsRegular() {
				continue
			}
			size := info.Size()
			if size < 0 {
				return nil, fmt.Errorf("resource %q has invalid size", filepath.ToSlash(rel))
			}
			if size > MaxResourceSize {
				return nil, fmt.Errorf("resource %q exceeds maximum size of %d bytes (actual: %d)", filepath.ToSlash(rel), MaxResourceSize, size)
			}
			if size > MaxTotalResourceBytes-totalBytes {
				return nil, fmt.Errorf("resource tree exceeds maximum total size of %d bytes", MaxTotalResourceBytes)
			}
			totalBytes += size
			perm := info.Mode().Perm()
			resources = append(resources, Resource{
				Path: filepath.ToSlash(rel), Size: size, Mode: fmt.Sprintf("%04o", perm), Executable: perm&0o111 != 0,
			})
		}
	}
	slices.SortFunc(resources, func(a, b Resource) int { return strings.Compare(a.Path, b.Path) })
	return resources, nil
}
