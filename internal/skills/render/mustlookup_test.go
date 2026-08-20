package render

import "testing"

func TestMustLookupSpecKnownTarget(t *testing.T) {
	spec := MustLookupSpec(TargetOpenCode)
	if spec.Name != TargetOpenCode {
		t.Fatalf("spec name = %q", spec.Name)
	}
}

func TestMustLookupSpecUnknownTargetPanics(t *testing.T) {
	defer func() {
		if r := recover(); r == nil {
			t.Fatal("expected panic for unknown target")
		}
	}()
	MustLookupSpec(Target("no-such-target"))
}
