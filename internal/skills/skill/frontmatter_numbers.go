package skill

import "gopkg.in/yaml.v3"

// The frozen Frontmatter decoder marshaled and reparsed its normalized map.
// Retain that numeric runtime normalization without re-emitting strings, whose
// leading LF must now survive. Scalar emission/resolution stays library-owned.
func normalizeMetadataNumber(value any) (any, error) {
	switch value := value.(type) {
	case float64:
		encoded, err := yaml.Marshal(value)
		if err != nil {
			return nil, err
		}
		var decoded any
		if err := yaml.Unmarshal(encoded, &decoded); err != nil {
			return nil, err
		}
		return decoded, nil
	case map[string]any:
		for key, item := range value {
			decoded, err := normalizeMetadataNumber(item)
			if err != nil {
				return nil, err
			}
			value[key] = decoded
		}
	case map[any]any:
		for key, item := range value {
			decoded, err := normalizeMetadataNumber(item)
			if err != nil {
				return nil, err
			}
			value[key] = decoded
		}
	case []any:
		for index, item := range value {
			decoded, err := normalizeMetadataNumber(item)
			if err != nil {
				return nil, err
			}
			value[index] = decoded
		}
	}
	return value, nil
}
