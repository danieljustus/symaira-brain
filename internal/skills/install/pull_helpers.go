package install

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/BurntSushi/toml"
	"github.com/danieljustus/symaira-brain/internal/skills/fsutil"
	"github.com/danieljustus/symaira-brain/internal/skills/render"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
	"gopkg.in/yaml.v3"
)

func bodyFromBundle(bundle *skill.Bundle) string { return strings.TrimLeft(bundle.Body, "\n") }

func pullBody(source, installed string, bundle *skill.Bundle, target render.Target, result *PullResult, out *string) error {
	prepend, appendText, err := overlayTexts(bundle, target)
	if err != nil {
		return err
	}
	prefix := ""
	if strings.TrimSpace(prepend) != "" {
		prefix = strings.TrimRight(prepend, "\n") + "\n\n"
	}
	suffix := ""
	if strings.TrimSpace(appendText) != "" {
		suffix = "\n\n" + strings.TrimRight(appendText, "\n")
	}
	installed = strings.TrimLeft(installed, "\n")
	if prefix != "" && !strings.HasPrefix(installed, prefix) {
		result.Refusals = append(result.Refusals, fmt.Sprintf("body prepend region changed (overlay %s)", overlayFile(bundle, target, "prepend.md")))
	}
	if suffix != "" && !strings.HasSuffix(strings.TrimRight(installed, "\n"), suffix) {
		result.Refusals = append(result.Refusals, fmt.Sprintf("body append region changed (overlay %s)", overlayFile(bundle, target, "append.md")))
	}
	if len(result.Refusals) > 0 {
		return fmt.Errorf("pull refused: %s", strings.Join(result.Refusals, "; "))
	}
	middle := installed
	if prefix != "" {
		middle = strings.TrimPrefix(middle, prefix)
	}
	if suffix != "" {
		middle = strings.TrimSuffix(strings.TrimRight(middle, "\n"), suffix)
	}
	middle = strings.TrimRight(middle, "\n") + "\n"
	if strings.TrimRight(middle, "\n") != strings.TrimRight(source, "\n") {
		result.Changes = append(result.Changes, PullChange{Path: "SKILL.md", Status: "modified"})
	}
	*out = middle
	return nil
}

func overlayTexts(bundle *skill.Bundle, target render.Target) (string, string, error) {
	cfg := bundle.Manifest.Targets[string(target)]
	prepend, err := overlayTextFor(bundle.Root, target, "prepend.md", cfg.Prepend)
	if err != nil {
		return "", "", err
	}
	appendText, err := overlayTextFor(bundle.Root, target, "append.md", cfg.Append)
	return prepend, appendText, err
}

func overlayTextFor(root string, target render.Target, name, configured string) (string, error) {
	path := configured
	if path == "" {
		dir := string(target)
		if spec, ok := render.LookupSpec(target); ok && spec.OverlayDir != "" {
			dir = spec.OverlayDir
		}
		path = filepath.Join("overlays", dir, name)
	}
	if filepath.IsAbs(path) || strings.HasPrefix(filepath.Clean(path), ".."+string(filepath.Separator)) {
		return "", fmt.Errorf("overlay reference %q escapes skill root", path)
	}
	data, err := os.ReadFile(filepath.Join(root, filepath.Clean(path)))
	if errors.Is(err, os.ErrNotExist) {
		return "", nil
	}
	return string(data), err
}

func overlayFile(bundle *skill.Bundle, target render.Target, name string) string {
	cfg := bundle.Manifest.Targets[string(target)]
	if name == "prepend.md" && cfg.Prepend != "" {
		return filepath.Join(bundle.Root, cfg.Prepend)
	}
	if name == "append.md" && cfg.Append != "" {
		return filepath.Join(bundle.Root, cfg.Append)
	}
	dir := string(target)
	if spec, ok := render.LookupSpec(target); ok && spec.OverlayDir != "" {
		dir = spec.OverlayDir
	}
	return filepath.Join(bundle.Root, "overlays", dir, name)
}

func pullFrontmatter(source, installed map[string]any, bundle *skill.Bundle, target render.Target, result *PullResult) error {
	owned, overlayValues, err := overlayFrontmatterKeys(bundle, target)
	if err != nil {
		return err
	}
	// Metadata is a map of independently-owned keys. Treating it as one
	// top-level value would either pull overlay metadata into the source or
	// refuse unrelated portable metadata changes.
	sourceMeta := mapValue(source["metadata"])
	installedMeta := mapValue(installed["metadata"])
	metaKeys := map[string]bool{}
	for k := range sourceMeta {
		metaKeys[k] = true
	}
	for k := range installedMeta {
		metaKeys[k] = true
	}
	for k := range metaKeys {
		key := "metadata." + k
		if equalAny(sourceMeta[k], installedMeta[k]) {
			continue
		}
		if owned[key] {
			if value, ok := overlayValues[key]; ok && equalAny(value, installedMeta[k]) {
				continue
			}
			result.Refusals = append(result.Refusals, fmt.Sprintf("frontmatter key %q is owned by target overlay", key))
			continue
		}
		from := sourceMeta[k]
		if installedMeta[k] == nil {
			delete(sourceMeta, k)
		} else {
			sourceMeta[k] = installedMeta[k]
		}
		result.FrontmatterChanges = append(result.FrontmatterChanges, PullFrontmatterChange{Key: key, From: from, To: installedMeta[k], Reason: "frontmatter"})
	}
	if len(sourceMeta) > 0 {
		source["metadata"] = sourceMeta
	} else {
		delete(source, "metadata")
	}

	keys := map[string]bool{}
	for k := range source {
		if k == "metadata" {
			continue
		}
		keys[k] = true
	}
	for k := range installed {
		if k == "metadata" {
			continue
		}
		keys[k] = true
	}
	for k := range keys {
		if equalAny(source[k], installed[k]) {
			continue
		}
		if owned[k] {
			// Target rendering always synthesizes compatibility and may
			// synthesize other configured values. Their unchanged rendered
			// value is not a harness edit; only a difference from that
			// baseline is a refusal.
			if value, ok := overlayValues[k]; ok && equalAny(value, installed[k]) {
				continue
			}
			result.Refusals = append(result.Refusals, fmt.Sprintf("frontmatter key %q is owned by target overlay", k))
			continue
		}
		from := source[k]
		if installed[k] == nil {
			delete(source, k)
		} else {
			source[k] = installed[k]
		}
		reason := "frontmatter"
		if k == "allowed-tools" || k == "allowed_tools" {
			reason = "permission-relevant frontmatter"
		}
		result.FrontmatterChanges = append(result.FrontmatterChanges, PullFrontmatterChange{Key: k, From: from, To: installed[k], Reason: reason})
	}
	if len(result.Refusals) > 0 {
		return fmt.Errorf("pull refused: %s", strings.Join(result.Refusals, "; "))
	}
	sort.Slice(result.FrontmatterChanges, func(i, j int) bool { return result.FrontmatterChanges[i].Key < result.FrontmatterChanges[j].Key })
	return nil
}

func mapValue(value any) map[string]any {
	out := map[string]any{}
	switch m := value.(type) {
	case map[string]any:
		for k, v := range m {
			out[k] = v
		}
	case map[string]string:
		for k, v := range m {
			out[k] = v
		}
	}
	return out
}

func overlayFrontmatterKeys(bundle *skill.Bundle, target render.Target) (map[string]bool, map[string]any, error) {
	owned := map[string]bool{"compatibility": true}
	values := map[string]any{"compatibility": string(target)}
	cfg := bundle.Manifest.Targets[string(target)]
	if cfg.Alias != "" {
		owned["name"] = true
		values["name"] = cfg.Alias
	}
	if cfg.Description != "" {
		owned["description"] = true
		values["description"] = cfg.Description
	}
	for k := range cfg.Metadata {
		owned["metadata."+k] = true
		values["metadata."+k] = cfg.Metadata[k]
	}
	path := filepath.Join(bundle.Root, "overlays", string(target), "frontmatter.toml")
	if spec, ok := render.LookupSpec(target); ok && spec.OverlayDir != "" {
		path = filepath.Join(bundle.Root, "overlays", spec.OverlayDir, "frontmatter.toml")
	}
	if _, err := os.Stat(path); errors.Is(err, os.ErrNotExist) {
		return owned, values, nil
	}
	var raw map[string]any
	if _, err := toml.DecodeFile(path, &raw); err != nil {
		return nil, nil, err
	}
	for k := range raw {
		if k == "metadata" {
			if m, ok := raw[k].(map[string]any); ok {
				for sub, value := range m {
					owned["metadata."+sub] = true
					values["metadata."+sub] = value
				}
			}
		} else {
			owned[k] = true
			values[k] = raw[k]
		}
	}
	return owned, values, nil
}

func equalAny(a, b any) bool {
	aa, _ := json.Marshal(a)
	bb, _ := json.Marshal(b)
	return string(aa) == string(bb)
}

func readSkillMarkdown(path string) (map[string]any, string, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, "", err
	}
	text := strings.ReplaceAll(string(data), "\r\n", "\n")
	if !strings.HasPrefix(text, "---\n") {
		return nil, "", fmt.Errorf("%s has no YAML frontmatter", path)
	}
	rest := strings.TrimPrefix(text, "---\n")
	end := strings.Index(rest, "\n---")
	if end < 0 {
		return nil, "", fmt.Errorf("%s frontmatter is not closed", path)
	}
	var fm map[string]any
	if err := yaml.Unmarshal([]byte(rest[:end]), &fm); err != nil {
		return nil, "", err
	}
	return fm, strings.TrimLeft(rest[end+len("\n---"):], "\n"), nil
}

func stagePullTree(stage, library, installed string, fm map[string]any, body string, changes []PullChange, target render.Target) error {
	if err := os.RemoveAll(stage); err != nil {
		return err
	}
	if err := fsutil.CopyTree(library, stage, func(rel string, d os.DirEntry) bool {
		return rel == pullManifestFile || (d.Name() == ".git" && d.IsDir())
	}); err != nil {
		return err
	}
	fmData, err := yaml.Marshal(fm)
	if err != nil {
		return err
	}
	skillData := append([]byte("---\n"), fmData...)
	skillData = append(skillData, []byte("---\n\n")...)
	skillData = append(skillData, []byte(body)...)
	if err := os.WriteFile(filepath.Join(stage, "SKILL.md"), skillData, 0o644); err != nil {
		return err
	}
	for _, change := range changes {
		if isNeverPulled(change.Path, target) || change.Path == "SKILL.md" {
			continue
		}
		dst := filepath.Join(stage, filepath.FromSlash(change.Path))
		src := filepath.Join(installed, filepath.FromSlash(change.Path))
		if change.Status == "removed" {
			if err := os.Remove(dst); err != nil && !errors.Is(err, os.ErrNotExist) {
				return err
			}
			continue
		}
		info, err := os.Stat(src)
		if err != nil {
			return err
		}
		if err := fsutil.CopyFile(src, dst, info.Mode().Perm()); err != nil {
			return err
		}
	}
	manifest := map[string]any{"target": string(target), "name": filepath.Base(stage), "changes": changes}
	data, _ := json.MarshalIndent(manifest, "", "  ")
	return os.WriteFile(filepath.Join(stage, pullManifestFile), append(data, '\n'), 0o644)
}

func isNeverPulled(path string, target render.Target) bool {
	if path == markerFile || path == pullManifestFile || path == codexMetadataFile {
		return true
	}
	if spec, ok := render.LookupSpec(target); ok && spec.MetadataFile != "" && path == filepath.ToSlash(spec.MetadataFile) {
		return true
	}
	return false
}

func unionPaths(maps ...map[string]string) []string {
	set := map[string]bool{}
	for _, m := range maps {
		for p := range m {
			set[p] = true
		}
	}
	out := make([]string, 0, len(set))
	for p := range set {
		out = append(out, p)
	}
	sort.Strings(out)
	return out
}

// ApplyPending promotes a previously staged pull into the library. It never
// installs to a harness target.
