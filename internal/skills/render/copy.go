package render

import (
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

func materializeStage(bundle *skill.Bundle, root *os.Root, stageRel string, item Rendered) error {
	resources := append([]skill.Resource(nil), bundle.Resources...)
	sort.Slice(resources, func(i, j int) bool { return resources[i].Path < resources[j].Path })
	for _, resource := range resources {
		if resource.Path == "SKILL.md" || resource.Path == ".symskills.json" || resource.Path == "symskills.toml" || strings.HasPrefix(resource.Path, "overlays/") || resource.Path == "overlays" {
			continue
		}
		if resource.Size > skill.MaxResourceSize {
			return fmt.Errorf("resource %q exceeds maximum size of %d bytes", resource.Path, skill.MaxResourceSize)
		}
		data, err := skill.ReadBundleBytes(bundle, resource.Path, skill.MaxResourceSize)
		if err != nil {
			return fmt.Errorf("copy %s: %w", resource.Path, err)
		}
		if replacement, ok := item.Files[resource.Path]; ok {
			data = []byte(replacement)
		}
		mode, parseErr := strconv.ParseUint(strings.TrimSpace(resource.Mode), 8, 32)
		if parseErr != nil {
			mode = 0o644
		}
		path := filepath.ToSlash(filepath.Join(stageRel, filepath.FromSlash(resource.Path)))
		if err := writeMaterializedFile(root, path, data, os.FileMode(mode)&0o777); err != nil {
			return fmt.Errorf("write %s: %w", resource.Path, err)
		}
	}
	for path, content := range item.Files {
		found := false
		for _, resource := range resources {
			if resource.Path == path {
				found = true
				break
			}
		}
		if !found {
			return fmt.Errorf("rendered resource %q is not in the source bundle", path)
		}
		_ = content
	}
	if err := writeMaterializedFile(root, filepath.ToSlash(filepath.Join(stageRel, "SKILL.md")), []byte(item.SkillMD), 0o644); err != nil {
		return fmt.Errorf("write SKILL.md: %w", err)
	}
	return writeTargetMetadataRoot(root, stageRel, item.Target, item)
}
