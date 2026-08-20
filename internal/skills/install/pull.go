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
	"gopkg.in/yaml.v3"

	"github.com/danieljustus/symaira-brain/internal/skills/fsutil"
	"github.com/danieljustus/symaira-brain/internal/skills/render"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
	"github.com/danieljustus/symaira-brain/internal/skills/vcs"
)

const (
	pullManifestFile  = ".symskills-pull.json"
	codexMetadataFile = "agents/openai.yaml"
)

// PullOptions configures a harness-to-library pull. Pull always writes a
// pending tree, never the library itself. ApplyPending is the explicit
// promotion step.
type PullOptions struct {
	HomeDir    string
	ProjectDir string
	Scope      render.Scope
	LibraryDir string
	PendingDir string
	BaseDir    string
	Target     render.Target
	Name       string
	DryRun     bool
}

// PullChange describes one content or resource change that would be staged.
type PullChange struct {
	Path   string `json:"path"`
	Status string `json:"status"` // modified, added, removed
}

// PullFrontmatterChange is kept separate from prose/resource changes because
// frontmatter can alter harness permissions, especially allowed-tools.
type PullFrontmatterChange struct {
	Key    string `json:"key"`
	From   any    `json:"from,omitempty"`
	To     any    `json:"to,omitempty"`
	Reason string `json:"reason,omitempty"`
}

// PullResult is the complete pull plan and staging result.
type PullResult struct {
	Action             string                  `json:"action"`
	Target             render.Target           `json:"target"`
	Name               string                  `json:"name"`
	StagePath          string                  `json:"stage_path,omitempty"`
	Changes            []PullChange            `json:"changes,omitempty"`
	FrontmatterChanges []PullFrontmatterChange `json:"frontmatter_changes,omitempty"`
	Refusals           []string                `json:"refusals,omitempty"`
}

// PendingPath returns the pending tree for target/name.
func PendingPath(target render.Target, name string, opts PullOptions) (string, error) {
	root := opts.PendingDir
	if root == "" {
		home := opts.HomeDir
		if home == "" {
			var err error
			home, err = os.UserHomeDir()
			if err != nil {
				return "", err
			}
		}
		root = filepath.Join(home, ".local", "share", "symskills", "pending")
	}
	if filepath.Base(name) != name || name == "." || name == ".." {
		return "", fmt.Errorf("invalid skill name %q", name)
	}
	return filepath.Join(root, string(target), name), nil
}

// Pull compares the installed target tree with the current library render and
// stages only harness-side changes. Overlay-produced regions are refused.
func Pull(opts PullOptions) (PullResult, error) {
	if opts.Scope == "" {
		opts.Scope = render.ScopeUser
	}
	if opts.Target == "" || opts.Name == "" {
		return PullResult{}, errors.New("pull requires target and skill name")
	}
	if opts.LibraryDir == "" {
		return PullResult{}, errors.New("pull requires library directory")
	}
	lock, err := AcquirePullLock(opts.Target, opts.Name, opts)
	if err != nil {
		return PullResult{Action: "locked", Target: opts.Target, Name: opts.Name}, err
	}
	defer lock.Release()
	libraryPath := filepath.Join(opts.LibraryDir, opts.Name)
	bundle, err := skill.LoadBundle(libraryPath)
	if err != nil {
		return PullResult{}, err
	}
	installedPath, err := InstallPath(opts.Target, opts.Name, Options{
		HomeDir: opts.HomeDir, ProjectDir: opts.ProjectDir, Scope: opts.Scope,
	})
	if err != nil {
		return PullResult{}, err
	}
	result := PullResult{Action: "planned", Target: opts.Target, Name: opts.Name, Changes: []PullChange{}, FrontmatterChanges: []PullFrontmatterChange{}, Refusals: []string{}}
	if _, err := os.Stat(installedPath); err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return result, fmt.Errorf("installed skill not found at %s", installedPath)
		}
		return result, err
	}

	// Render into a throwaway tree. Besides giving us the exact fresh output,
	// this avoids touching the live render cache in symlink mode.
	rendered, cleanup, err := render.StagingRender(bundle, []render.Target{opts.Target})
	if err != nil {
		return result, err
	}
	defer cleanup()
	if len(rendered) == 0 {
		return result, fmt.Errorf("target %s produced no render output", opts.Target)
	}
	freshPath := rendered[0].Path

	base, _ := pullBaseHashes(opts)
	left, err := fileHashes(freshPath)
	if err != nil {
		return result, err
	}
	right, err := fileHashes(installedPath)
	if err != nil {
		return result, err
	}
	var driftOutcome DriftOutcome
	if len(base) > 0 {
		drifts := ClassifyDrift(base, left, right)
		driftOutcome = SummarizeDrift(drifts)
		for path := range driftOutcome.Refusable {
			result.Refusals = append(result.Refusals, fmt.Sprintf("conflict in %s", path))
		}
		if len(result.Refusals) > 0 {
			sort.Strings(result.Refusals)
			return result, fmt.Errorf("pull refused: %s", strings.Join(result.Refusals, "; "))
		}
	}

	sourceFM, sourceBody, err := readSkillMarkdown(filepath.Join(libraryPath, "SKILL.md"))
	if err != nil {
		return result, err
	}
	installedFM, installedBody, err := readSkillMarkdown(filepath.Join(installedPath, "SKILL.md"))
	if err != nil {
		return result, err
	}
	if err := pullBody(sourceBody, installedBody, bundle, opts.Target, &result, &sourceBody); err != nil {
		return result, err
	}
	if err := pullFrontmatter(sourceFM, installedFM, bundle, opts.Target, &result); err != nil {
		return result, err
	}

	// Copy resource changes from the harness tree. Marker and generated target
	// metadata are bookkeeping/output, not portable source files.
	for _, path := range unionPaths(left, right, base) {
		if isNeverPulled(path, opts.Target) || path == "SKILL.md" {
			continue
		}
		ld, lok := left[path]
		rd, rok := right[path]
		bok := false
		if len(base) > 0 {
			_, bok = base[path]
		}
		if len(base) > 0 && !driftOutcome.Pullable[path] {
			continue
		}
		if !rok {
			if bok && lok {
				result.Changes = append(result.Changes, PullChange{Path: path, Status: "removed"})
			}
			continue
		}
		if !lok || ld != rd {
			status := "modified"
			if !lok {
				status = "added"
			}
			result.Changes = append(result.Changes, PullChange{Path: path, Status: status})
		}
	}
	sort.Slice(result.Changes, func(i, j int) bool { return result.Changes[i].Path < result.Changes[j].Path })
	if len(result.Changes) == 0 && sourceBody == bodyFromBundle(bundle) && len(result.FrontmatterChanges) == 0 {
		result.Action = "current"
	}
	if opts.DryRun {
		result.Action = "planned"
		return result, nil
	}
	stage, err := PendingPath(opts.Target, opts.Name, opts)
	if err != nil {
		return result, err
	}
	if err := stagePullTree(stage, libraryPath, installedPath, sourceFM, sourceBody, result.Changes, opts.Target); err != nil {
		return result, err
	}
	result.StagePath = stage
	result.Action = "staged"
	return result, nil
}

func pullBaseHashes(opts PullOptions) (map[string]string, error) {
	baseDir, err := BasePath(opts.Target, opts.Name, Options{HomeDir: opts.HomeDir, ProjectDir: opts.ProjectDir, Scope: opts.Scope, BaseDir: opts.BaseDir})
	if err != nil {
		return nil, err
	}
	return baseHashes(baseDir)
}

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
func ApplyPending(opts PullOptions) error {
	lock, err := AcquirePullLock(opts.Target, opts.Name, opts)
	if err != nil {
		return err
	}
	defer lock.Release()
	stage, err := PendingPath(opts.Target, opts.Name, opts)
	if err != nil {
		return err
	}
	if _, err := os.Stat(filepath.Join(stage, pullManifestFile)); err != nil {
		return fmt.Errorf("no pending pull for %s/%s", opts.Target, opts.Name)
	}
	if opts.LibraryDir == "" {
		return errors.New("apply requires library directory")
	}
	target := filepath.Join(opts.LibraryDir, opts.Name)
	tmp := target + ".pull-tmp-" + fmt.Sprint(os.Getpid())
	if err := os.RemoveAll(tmp); err != nil {
		return err
	}
	defer os.RemoveAll(tmp)
	if err := fsutil.CopyTree(stage, tmp, func(rel string, d os.DirEntry) bool {
		return rel == pullManifestFile || (d.Name() == ".git" && d.IsDir())
	}); err != nil {
		return err
	}
	entries, err := os.ReadDir(target)
	if err != nil {
		return err
	}
	for _, entry := range entries {
		if entry.Name() == ".git" {
			continue
		}
		if err := os.RemoveAll(filepath.Join(target, entry.Name())); err != nil {
			return err
		}
	}
	tmpEntries, err := os.ReadDir(tmp)
	if err != nil {
		return err
	}
	for _, entry := range tmpEntries {
		if err := os.Rename(filepath.Join(tmp, entry.Name()), filepath.Join(target, entry.Name())); err != nil {
			return err
		}
	}
	if vcs.IsRepo(target) {
		if _, err := vcs.Commit(target, fmt.Sprintf("pull: apply pending changes for %s", opts.Name)); err != nil && !errors.Is(err, vcs.ErrUnavailable) {
			return err
		}
	}
	return os.RemoveAll(stage)
}
