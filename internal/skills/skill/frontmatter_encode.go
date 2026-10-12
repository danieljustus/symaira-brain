package skill

import (
	"strings"

	"gopkg.in/yaml.v3"
)

// frontmatterString selects a lossless library style for the approved #835
// exception: yaml.v3's literal emitter consumes a leading LF as its header break.
type frontmatterString string

type frontmatterKey string

func (value frontmatterKey) MarshalYAML() (any, error) {
	if value == "<<" {
		return &yaml.Node{Kind: yaml.ScalarNode, Tag: "!!str", Value: string(value), Style: yaml.DoubleQuotedStyle}, nil
	}
	return frontmatterString(value).MarshalYAML()
}

func (value frontmatterString) MarshalYAML() (any, error) {
	if strings.HasPrefix(string(value), "\n") {
		return &yaml.Node{Kind: yaml.ScalarNode, Tag: "!!str", Value: string(value), Style: yaml.DoubleQuotedStyle}, nil
	}
	return string(value), nil
}

// MarshalYAML retains the existing field order and omitempty contract while
// delegating scalar/collection emission and natural map ordering to yaml.v3.
func (fm Frontmatter) MarshalYAML() (any, error) {
	return struct {
		Name                         frontmatterString   `yaml:"name"`
		Description                  frontmatterString   `yaml:"description"`
		Category                     frontmatterString   `yaml:"category,omitempty"`
		Version                      frontmatterString   `yaml:"version,omitempty"`
		Author                       frontmatterString   `yaml:"author,omitempty"`
		License                      frontmatterString   `yaml:"license,omitempty"`
		Compatibility                frontmatterString   `yaml:"compatibility,omitempty"`
		Platforms                    []frontmatterString `yaml:"platforms,omitempty"`
		RequiredEnvironmentVariables []frontmatterString `yaml:"required_environment_variables,omitempty"`
		Metadata                     map[any]any         `yaml:"metadata,omitempty"`
	}{
		frontmatterString(fm.Name), frontmatterString(fm.Description), frontmatterString(fm.Category),
		frontmatterString(fm.Version), frontmatterString(fm.Author), frontmatterString(fm.License),
		frontmatterString(fm.Compatibility), frontmatterStrings(fm.Platforms),
		frontmatterStrings(fm.RequiredEnvironmentVariables), frontmatterMetadata(fm.Metadata),
	}, nil
}

func frontmatterStrings(values []string) []frontmatterString {
	if values == nil {
		return nil
	}
	result := make([]frontmatterString, len(values))
	for i, value := range values {
		result[i] = frontmatterString(value)
	}
	return result
}

func frontmatterMetadata(values map[string]any) map[any]any {
	result := make(map[any]any, len(values))
	for key, value := range values {
		result[frontmatterKey(key)] = frontmatterMetadataValue(value)
	}
	return result
}

func frontmatterMetadataValue(value any) any {
	switch value := value.(type) {
	case string:
		return frontmatterString(value)
	case []string:
		return frontmatterStrings(value)
	case []any:
		result := make([]any, len(value))
		for i, item := range value {
			result[i] = frontmatterMetadataValue(item)
		}
		return result
	case map[string]any:
		return frontmatterMetadata(value)
	case map[any]any:
		result := make(map[any]any, len(value))
		for key, item := range value {
			if text, ok := key.(string); ok {
				key = frontmatterKey(text)
			}
			result[key] = frontmatterMetadataValue(item)
		}
		return result
	default:
		return value
	}
}
