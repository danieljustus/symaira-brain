package render

import "testing"

// TestAntigravityTargetPaths pins where Antigravity skills are installed.
//
// This is not a formality: skills used to be installed into
// ~/.gemini/antigravity-cli/skills, a per-client state directory that
// Antigravity never reads, so they were rendered and symlinked correctly and
// silently ignored. The global root is the shared config directory that both
// the app and the agy CLI read; the project root is shared with Codex and
// OpenClaw. A wrong path here fails silently at runtime, which is exactly the
// failure mode a test has to catch.
func TestAntigravityTargetPaths(t *testing.T) {
	spec, ok := LookupSpec(TargetAntigravity)
	if !ok {
		t.Fatal("antigravity target not found in the registry")
	}
	if spec.BinaryName != "agy" {
		t.Errorf("binary name = %q, want agy (the Gemini CLI is retired)", spec.BinaryName)
	}

	cases := []struct {
		name string
		got  string
		want string
	}{
		{"user skill root", spec.SkillRoot("/home/u", "/proj", ScopeUser), "/home/u/.gemini/config/skills"},
		{"project skill root", spec.SkillRoot("/home/u", "/proj", ScopeProject), "/proj/.agents/skills"},
		{"user config dir", spec.ConfigDir("/home/u", "/proj", ScopeUser), "/home/u/.gemini/config"},
		{"project config dir", spec.ConfigDir("/home/u", "/proj", ScopeProject), "/proj/.agents"},
		// With no project directory the project scope must fall back to the
		// user paths rather than producing a root-relative path.
		{"project scope without project dir", spec.SkillRoot("/home/u", "", ScopeProject), "/home/u/.gemini/config/skills"},
		{"project config without project dir", spec.ConfigDir("/home/u", "", ScopeProject), "/home/u/.gemini/config"},
	}
	for _, tc := range cases {
		if tc.got != tc.want {
			t.Errorf("%s = %q, want %q", tc.name, tc.got, tc.want)
		}
	}
}
