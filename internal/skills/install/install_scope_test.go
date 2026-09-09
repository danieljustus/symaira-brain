package install

import (
	"path/filepath"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/render"
)

func TestInstallPathUserAllTargets(t *testing.T) {
	home := t.TempDir()
	opts := Options{HomeDir: home, Scope: render.ScopeUser}

	cases := []struct {
		target render.Target
		sub    []string
	}{
		{render.TargetOpenCode, []string{".config", "opencode", "skills", "my-skill"}},
		{render.TargetClaude, []string{".claude", "skills", "my-skill"}},
		{render.TargetCodex, []string{".agents", "skills", "my-skill"}},
		{render.TargetHermes, []string{".hermes", "skills", "symaira", "my-skill"}},
		{render.TargetAntigravity, []string{".gemini", "config", "skills", "my-skill"}},
		{render.TargetOpenClaw, []string{".openclaw", "skills", "my-skill"}},
	}
	for _, c := range cases {
		got, err := InstallPath(c.target, "my-skill", opts)
		if err != nil {
			t.Fatalf("InstallPath(%s, ScopeUser): %v", c.target, err)
		}
		want := filepath.Join(append([]string{home}, c.sub...)...)
		if got != want {
			t.Errorf("InstallPath(%s, ScopeUser) = %q, want %q", c.target, got, want)
		}
	}

	// unknown target with user scope
	_, err := InstallPath("unknown-target", "my-skill", opts)
	if err == nil {
		t.Fatal("expected error for unknown target with user scope")
	}
}

func TestTargetDir(t *testing.T) {
	home := t.TempDir()
	opts := Options{HomeDir: home, Scope: render.ScopeUser}

	cases := []struct {
		target render.Target
		want   string
	}{
		{render.TargetOpenCode, filepath.Join(home, ".config", "opencode", "skills")},
		{render.TargetClaude, filepath.Join(home, ".claude", "skills")},
		{render.TargetCodex, filepath.Join(home, ".agents", "skills")},
		{render.TargetHermes, filepath.Join(home, ".hermes", "skills", "symaira")},
		{render.TargetAntigravity, filepath.Join(home, ".gemini", "config", "skills")},
		{render.TargetOpenClaw, filepath.Join(home, ".openclaw", "skills")},
	}

	for _, c := range cases {
		got, err := TargetDir(c.target, opts)
		if err != nil {
			t.Fatalf("TargetDir(%s): %v", c.target, err)
		}
		if got != c.want {
			t.Errorf("TargetDir(%s) = %q, want %q", c.target, got, c.want)
		}
	}
}
