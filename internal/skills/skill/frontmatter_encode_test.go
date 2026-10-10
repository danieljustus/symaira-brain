package skill

import (
	"strings"
	"testing"

	"gopkg.in/yaml.v3"
)

func TestFrontmatterLeadingBreaksRoundTrip(t *testing.T) {
	for _, value := range []string{"\n", "\n\n", "\n\nX", "<<", "ordinary", "line\nnext", "line\n\x00tail", "line\r\ntail"} {
		t.Run(value, func(t *testing.T) {
			if value == "<<" {
				legacy, err := yaml.Marshal(map[string]string{"<<": "marker"})
				if err != nil {
					t.Fatal(err)
				}
				var parsed map[string]string
				if err := yaml.Unmarshal(legacy, &parsed); err == nil || !strings.Contains(err.Error(), "map merge requires map or sequence") {
					t.Fatalf("legacy merge-key rejection changed: %v", err)
				}
			}
			if strings.HasPrefix(value, "\n") {
				// Execute yaml.v3's unchanged string path separately: its
				// literal emitter loses one leading LF. Never bless that loss.
				legacy, err := yaml.Marshal(map[string]string{"value": value})
				if err != nil {
					t.Fatal(err)
				}
				var parsed map[string]string
				if err := yaml.Unmarshal(legacy, &parsed); err != nil {
					t.Fatal(err)
				}
				if parsed["value"] != strings.TrimPrefix(value, "\n") {
					t.Fatalf("legacy emitter observation changed: %q", parsed["value"])
				}
			}
			fm := Frontmatter{
				Name: "leading-break-control", Description: value,
				Category: value, Version: value, Author: value, License: value,
				Compatibility: value, Platforms: []string{value}, RequiredEnvironmentVariables: []string{value},
				Metadata: map[string]any{value: value, "nested": map[string]any{"value": value}, "list": []any{value}},
			}
			raw, err := yaml.Marshal(fm)
			if err != nil {
				t.Fatal(err)
			}
			var parsed Frontmatter
			if err := yaml.Unmarshal(raw, &parsed); err != nil {
				t.Fatal(err)
			}
			if parsed.Description != value || parsed.Metadata[value] != value {
				t.Fatalf("lost scalar/key: description=%q metadata=%#v, want %q", parsed.Description, parsed.Metadata, value)
			}
			if parsed.Category != value || parsed.Version != value || parsed.Author != value || parsed.License != value || parsed.Compatibility != value || parsed.Platforms[0] != value || parsed.RequiredEnvironmentVariables[0] != value {
				t.Fatalf("lost frontmatter string field: %#v, want %q", parsed, value)
			}
			if parsed.Metadata["nested"].(map[string]any)["value"] != value || parsed.Metadata["list"].([]any)[0] != value {
				t.Fatalf("lost nested scalar: %#v, want %q", parsed.Metadata, value)
			}
		})
	}
}
