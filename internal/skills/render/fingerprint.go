package render

import (
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"sort"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

// sourceFingerprint hashes every loaded source file through the retained
// bundle capability. The loader has already inventoried the complete tree,
// including overlays and control files, so this avoids reopening bundle.Root.
func sourceFingerprint(bundle *skill.Bundle) (string, error) {
	if bundle == nil {
		return "", errors.New("bundle is nil")
	}
	h := sha256.New()
	paths := make([]string, 0, len(bundle.Resources)+1)
	paths = append(paths, "SKILL.md")
	modes := make(map[string]string, len(bundle.Resources))
	for _, resource := range bundle.Resources {
		paths = append(paths, resource.Path)
		modes[resource.Path] = resource.Mode
	}
	sort.Strings(paths)
	for _, path := range paths {
		limit := int64(skill.MaxResourceSize)
		if path == "SKILL.md" || path == "symskills.toml" {
			limit = skill.MaxInputSize
		}
		data, err := skill.ReadBundleBytes(bundle, path, limit)
		if err != nil {
			return "", err
		}
		h.Write([]byte(path))
		h.Write([]byte{0})
		h.Write([]byte(modes[path]))
		h.Write([]byte{0})
		h.Write(data)
		h.Write([]byte{0})
	}
	for _, spec := range Targets {
		if spec.MetadataTemplate == "" {
			continue
		}
		h.Write([]byte("metadata-template"))
		h.Write([]byte{0})
		h.Write([]byte(spec.Name))
		h.Write([]byte{0})
		h.Write([]byte(metadataTemplateFingerprint(spec.Name)))
		h.Write([]byte{0})
	}
	return hex.EncodeToString(h.Sum(nil)), nil
}

// sourceTreeHash computes a content hash of support files through the
// capability retained by the loader. It deliberately does not reopen or walk
// bundle.Root, so the hash and the later copy observe the same root handle.
func sourceTreeHash(bundle *skill.Bundle) string {
	if bundle == nil {
		return ""
	}
	resources := append([]skill.Resource(nil), bundle.Resources...)
	sort.Slice(resources, func(i, j int) bool { return resources[i].Path < resources[j].Path })
	h := sha256.New()
	for _, resource := range resources {
		if resource.Path == ".symskills.json" || resource.Path == "symskills.toml" || strings.HasPrefix(resource.Path, "overlays/") || resource.Path == "overlays" {
			continue
		}
		data, err := skill.ReadBundleBytes(bundle, resource.Path, skill.MaxResourceSize)
		if err != nil {
			return ""
		}
		h.Write([]byte(resource.Path))
		h.Write([]byte{0})
		h.Write([]byte(resource.Mode))
		h.Write([]byte{0})
		h.Write(data)
		h.Write([]byte{0})
	}
	return hex.EncodeToString(h.Sum(nil))
}

// sourceHash combines the once-per-bundle source tree hash with the
// per-target rendered SKILL.md content and target name so re-renders with
// unchanged input can be skipped. The tree hash covers the raw support
// files, so resolved markdown resources — whose content depends on this
// target's block overrides and terms — are mixed in separately. That mix is
// skipped when the skill resolves no variants, keeping the hash of an
// ordinary skill exactly what it was before harness variants existed.
func sourceHash(treeHash, renderedSkillMD string, target Target, files map[string]string) string {
	return sourceHashWithMetadata(treeHash, renderedSkillMD, target, files, "", nil)
}

func sourceHashWithMetadata(treeHash, renderedSkillMD string, target Target, files map[string]string, metadataFile string, metadataBytes []byte) string {
	h := sha256.New()
	h.Write([]byte(treeHash))
	h.Write([]byte{0})
	h.Write([]byte(renderedSkillMD))
	h.Write([]byte{0})
	h.Write([]byte(target))
	if metadataFile != "" {
		h.Write([]byte{0})
		h.Write([]byte(metadataFile))
		h.Write([]byte{0})
		h.Write(metadataBytes)
	} else if templateHash := metadataTemplateFingerprint(target); templateHash != "" {
		h.Write([]byte{0})
		h.Write([]byte(templateHash))
	}
	if vh := variantFilesHash(files); vh != "" {
		h.Write([]byte{0})
		h.Write([]byte(vh))
	}
	return hex.EncodeToString(h.Sum(nil))
}

func metadataTemplateFingerprint(target Target) string {
	spec, ok := LookupSpec(target)
	if !ok || spec.MetadataTemplate == "" {
		return ""
	}
	data, err := skill.ReadExternalBytes(spec.MetadataTemplate, skill.MaxInputSize)
	if err != nil {
		return spec.MetadataTemplate + "\x00error:" + err.Error()
	}
	h := sha256.New()
	h.Write([]byte(spec.MetadataTemplate))
	h.Write([]byte{0})
	h.Write(data)
	return hex.EncodeToString(h.Sum(nil))
}

// variantFilesHash hashes the resolved markdown resources in path order. An
// empty map yields an empty hash so it contributes nothing.
func variantFilesHash(files map[string]string) string {
	if len(files) == 0 {
		return ""
	}
	paths := make([]string, 0, len(files))
	for path := range files {
		paths = append(paths, path)
	}
	sort.Strings(paths)
	h := sha256.New()
	for _, path := range paths {
		h.Write([]byte(path))
		h.Write([]byte{0})
		h.Write([]byte(files[path]))
		h.Write([]byte{0})
	}
	return hex.EncodeToString(h.Sum(nil))
}
