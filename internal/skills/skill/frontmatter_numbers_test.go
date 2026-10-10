package skill

import (
	"math"
	"reflect"
	"testing"

	"gopkg.in/yaml.v3"
)

func TestFrontmatterKeepsLegacyNumericNormalizationWithoutStringLoss(t *testing.T) {
	input := "name: probe\ndescription: \"\\n\\nX\"\nmetadata:\n  values: [-0.0, 1.0, 1e6, 1e-10]\n  nested: {\"<<\": \"\\n\"}\n"
	var fm Frontmatter
	if err := yaml.Unmarshal([]byte(input), &fm); err != nil {
		t.Fatal(err)
	}
	// Genuine negative zero, not a Go constant folded to positive zero.
	values := []float64{math.Copysign(0, -1), 1, 1e6, 1e-10}
	if !math.Signbit(values[0]) {
		t.Fatal("control is not negative zero")
	}
	var expected []any
	for _, value := range values {
		encoded, err := yaml.Marshal(value)
		if err != nil {
			t.Fatal(err)
		}
		var parsed any
		if err := yaml.Unmarshal(encoded, &parsed); err != nil {
			t.Fatal(err)
		}
		expected = append(expected, parsed)
	}
	if !reflect.DeepEqual(fm.Metadata["values"], expected) {
		t.Fatalf("numeric kinds/values: %#v != %#v", fm.Metadata["values"], expected)
	}
	if fm.Description != "\n\nX" || fm.Metadata["nested"].(map[string]any)["<<"] != "\n" {
		t.Fatal("lossless strings were re-emitted")
	}
}
