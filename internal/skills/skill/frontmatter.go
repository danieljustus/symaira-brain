// Package skill loads, validates, and imports portable Agent Skill bundles.
package skill

import (
	"fmt"
	"path/filepath"
	"strings"

	"gopkg.in/yaml.v3"
)

// UnmarshalYAML accepts scalar values and YAML string lists for fields that
// are represented as strings in the public Frontmatter type.
func (fm *Frontmatter) UnmarshalYAML(value *yaml.Node) error {
	var fields map[string]yaml.Node
	if err := value.Decode(&fields); err != nil {
		return err
	}

	normalized := make(map[string]any, len(fields))
	for name, node := range fields {
		if isStringFrontmatterField(name) {
			text, err := decodeStringFrontmatterField(name, node)
			if err != nil {
				return err
			}
			normalized[name] = text
			continue
		}
		var field any
		if err := node.Decode(&field); err != nil {
			return fmt.Errorf("frontmatter field %q: %w", name, err)
		}
		normalized[name] = field
	}

	data, err := yaml.Marshal(normalized)
	if err != nil {
		return fmt.Errorf("normalize frontmatter: %w", err)
	}
	type frontmatter Frontmatter
	var decoded frontmatter
	if err := yaml.Unmarshal(data, &decoded); err != nil {
		return err
	}
	*fm = Frontmatter(decoded)
	return nil
}

func isStringFrontmatterField(name string) bool {
	switch name {
	case "name", "description", "category", "version", "author", "license", "compatibility":
		return true
	default:
		return false
	}
}

func decodeStringFrontmatterField(name string, node yaml.Node) (string, error) {
	if node.Kind == 0 || node.Tag == "!!null" {
		return "", nil
	}
	if node.Kind == yaml.ScalarNode {
		var text string
		if err := node.Decode(&text); err != nil {
			return "", fmt.Errorf("frontmatter field %q must be a string or a list of strings: %w", name, err)
		}
		return text, nil
	}
	if node.Kind == yaml.SequenceNode {
		var values []string
		if err := node.Decode(&values); err != nil {
			return "", fmt.Errorf("frontmatter field %q must be a string or a list of strings: %w", name, err)
		}
		return strings.Join(values, ", "), nil
	}
	return "", fmt.Errorf("frontmatter field %q must be a string or a list of strings", name)
}

func parseSkillMD(raw []byte) (Frontmatter, string, error) {
	text := strings.ReplaceAll(string(raw), "\r\n", "\n")
	if !strings.HasPrefix(text, "---\n") {
		return Frontmatter{}, "", fmt.Errorf("SKILL.md must start with YAML frontmatter")
	}
	rest := strings.TrimPrefix(text, "---\n")
	end := strings.Index(rest, "\n---")
	if end < 0 {
		return Frontmatter{}, "", fmt.Errorf("SKILL.md frontmatter is not closed")
	}
	fmText := rest[:end]
	if len(fmText) > MaxFrontmatterSize {
		return Frontmatter{}, "", fmt.Errorf("SKILL.md frontmatter exceeds maximum size of %d bytes", MaxFrontmatterSize)
	}
	body := rest[end+len("\n---"):]
	body = strings.TrimLeft(body, "\n")

	var fm Frontmatter
	if err := yaml.Unmarshal([]byte(fmText), &fm); err != nil {
		return Frontmatter{}, "", fmt.Errorf("parse SKILL.md frontmatter: %w", err)
	}
	if fm.Metadata == nil {
		fm.Metadata = map[string]any{}
	}
	fm.Category = NormalizeCategory(fm.Category)
	return fm, body, nil
}

// NormalizeCategory trims and collapses whitespace without changing the
// display spelling. Library-level canonicalization is handled by
// NormalizeCategories, which can snap case variants to one existing spelling.
func NormalizeCategory(category string) string {
	return strings.Join(strings.Fields(category), " ")
}

// NormalizeCategories canonicalizes category spelling across a loaded library.
// The first non-empty spelling wins; later case-insensitive variants reuse it.
func NormalizeCategories(bundles []*Bundle) {
	canonical := map[string]string{}
	for _, bundle := range bundles {
		if bundle == nil {
			continue
		}
		category := NormalizeCategory(bundle.Frontmatter.Category)
		if category == "" {
			bundle.Frontmatter.Category = ""
			continue
		}
		key := strings.ToLower(category)
		if existing, ok := canonical[key]; ok {
			bundle.Frontmatter.Category = existing
			continue
		}
		canonical[key] = category
		bundle.Frontmatter.Category = category
	}
}

// loadFrontmatterOnly reads SKILL.md's YAML frontmatter without allocating
// the (often much larger) Markdown body, for callers that only need the
// header metadata (e.g. ListLibrary).
func loadFrontmatterOnly(root string) (Frontmatter, error) {
	abs, err := filepath.Abs(root)
	if err != nil {
		return Frontmatter{}, err
	}
	rootCap, err := openBundleRoot(abs)
	if err != nil {
		return Frontmatter{}, err
	}
	defer rootCap.Close()
	raw, err := readRootFile(rootCap, "SKILL.md", "SKILL.md", MaxInputSize)
	if err != nil {
		return Frontmatter{}, err
	}
	fm, _, err := parseSkillMD(raw)
	return fm, err
}
