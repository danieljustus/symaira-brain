package mcptools

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/events"
	"github.com/danieljustus/symaira-corekit/mcpserver"
)

func writeTestSkill(t *testing.T, dir string) {
	t.Helper()
	content := `---
name: test-skill
description: Test skill for unit tests
---
# Test Skill

Test description
`
	if err := os.WriteFile(filepath.Join(dir, "SKILL.md"), []byte(content), 0644); err != nil {
		t.Fatalf("write SKILL.md: %v", err)
	}
}

func writeTestProfile(t *testing.T, dir, name, skill string) {
	t.Helper()
	content := fmt.Sprintf("name = %q\n\n[links]\n%s = { skill = %q }\n", name, skill, skill)
	if err := os.WriteFile(filepath.Join(dir, name+".toml"), []byte(content), 0644); err != nil {
		t.Fatalf("write profile: %v", err)
	}
}

// parseText decodes a text content field that was produced by mcpJSON.
// After the serialization fix, text is a JSON string containing the
// serialized result (e.g. `"{\"skills\":[...]}"`). This helper
// unmarshals the JSON string first, then unmarshals the inner JSON.
func parseText(raw json.RawMessage, v any) error {
	var text string
	if err := json.Unmarshal(raw, &text); err != nil {
		return fmt.Errorf("unmarshal text string: %w", err)
	}
	return json.Unmarshal([]byte(text), v)
}

func TestRegisterExposesExpectedTools(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	Register(srv, Options{})

	req := `{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}` + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			Tools []struct {
				Name        string                     `json:"name"`
				Annotations map[string]json.RawMessage `json:"annotations"`
			} `json:"tools"`
		} `json:"result"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response %q: %v", out.String(), err)
	}
	names := map[string]bool{}
	readOnly := map[string]bool{
		"skills_list": true, "skills_inspect": true, "skills_validate": true,
		"skills_profile_list": true, "skills_profile_resolve": true,
		"skills_targets_status": true, "skills_discover_sources": true,
		"skills_history": true,
	}
	for _, tool := range resp.Result.Tools {
		names[tool.Name] = true
		if len(tool.Annotations) == 0 {
			t.Errorf("tool %q is missing annotations in tools/list", tool.Name)
		}
		if readOnly[tool.Name] {
			var got bool
			raw, ok := tool.Annotations["readOnlyHint"]
			if !ok || json.Unmarshal(raw, &got) != nil || !got {
				t.Errorf("tool %q readOnlyHint = %s, want true", tool.Name, raw)
			}
		}
	}
	for _, want := range []string{"skills_list", "skills_inspect", "skills_validate", "skills_profile_list", "skills_profile_resolve", "skills_render_plan", "skills_install", "skills_targets_status", "skills_discover_sources", "skills_history", "skills_restore"} {
		if !names[want] {
			t.Fatalf("missing MCP tool %s in %#v", want, names)
		}
	}
}

func TestSkillsListEmptyLibrary(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	Register(srv, Options{LibraryDir: filepath.Join(tmpDir, "empty-lib")})

	req := `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_list","arguments":{}}}` + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			Content []struct {
				Type string          `json:"type"`
				Text json.RawMessage `json:"text"`
			} `json:"content"`
		} `json:"result"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response %q: %v", out.String(), err)
	}

	if len(resp.Result.Content) == 0 {
		t.Fatal("expected content in response")
	}

	var result struct {
		Skills []any `json:"skills"`
		Issues []any `json:"issues"`
	}
	if err := parseText(resp.Result.Content[0].Text, &result); err != nil {
		t.Fatalf("parse result: %v", err)
	}

	if len(result.Skills) != 0 {
		t.Errorf("expected 0 skills, got %d", len(result.Skills))
	}
}

func TestSkillsInspectMissingPath(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	Register(srv, Options{LibraryDir: filepath.Join(tmpDir, "lib")})

	req := `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_inspect","arguments":{"path":"/nonexistent/path"}}}` + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			IsError bool `json:"isError"`
		} `json:"result"`
		Error *struct {
			Code    int    `json:"code"`
			Message string `json:"message"`
		} `json:"error"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response %q: %v", out.String(), err)
	}

	if resp.Error == nil && !resp.Result.IsError {
		t.Fatal("expected error for missing path")
	}
}

func TestSkillsValidateMissingPath(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	Register(srv, Options{LibraryDir: filepath.Join(tmpDir, "lib")})

	req := `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_validate","arguments":{"path":"/nonexistent/path"}}}` + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			IsError bool `json:"isError"`
		} `json:"result"`
		Error *struct {
			Code    int    `json:"code"`
			Message string `json:"message"`
		} `json:"error"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response %q: %v", out.String(), err)
	}

	if resp.Error == nil && !resp.Result.IsError {
		t.Fatal("expected error for missing path")
	}
}

func TestSkillsRenderPlanMissingPath(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	Register(srv, Options{LibraryDir: filepath.Join(tmpDir, "lib"), RenderDir: filepath.Join(tmpDir, "render")})

	req := `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_render_plan","arguments":{"path":"/nonexistent/path"}}}` + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			IsError bool `json:"isError"`
		} `json:"result"`
		Error *struct {
			Code    int    `json:"code"`
			Message string `json:"message"`
		} `json:"error"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response %q: %v", out.String(), err)
	}

	if resp.Error == nil && !resp.Result.IsError {
		t.Fatal("expected error for missing path")
	}
}

func TestSkillsInstallMissingPath(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	Register(srv, Options{
		LibraryDir: filepath.Join(tmpDir, "lib"),
		RenderDir:  filepath.Join(tmpDir, "render"),
		HomeDir:    tmpDir,
	})

	req := `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_install","arguments":{"path":"/nonexistent/path"}}}` + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			IsError bool `json:"isError"`
		} `json:"result"`
		Error *struct {
			Code    int    `json:"code"`
			Message string `json:"message"`
		} `json:"error"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response %q: %v", out.String(), err)
	}

	if resp.Error == nil && !resp.Result.IsError {
		t.Fatal("expected error for missing path")
	}
}

func TestCallInspect(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()

	skillDir := filepath.Join(tmpDir, "test-skill")
	os.MkdirAll(skillDir, 0755)
	skillContent := `---
name: test-skill
description: Test skill for unit tests
---
# Test Skill

Test description
`
	os.WriteFile(filepath.Join(skillDir, "SKILL.md"), []byte(skillContent), 0644)

	Register(srv, Options{LibraryDir: filepath.Join(tmpDir, "lib")})

	in := json.RawMessage(`{"path":"` + skillDir + `"}`)
	bundle, err := callInspect(context.Background(), srv, Options{LibraryDir: filepath.Join(tmpDir, "lib")}, in)
	if err != nil {
		t.Fatalf("callInspect failed: %v", err)
	}
	if bundle == nil {
		t.Fatal("expected bundle, got nil")
	}

	in = json.RawMessage(`{"name":"nonexistent"}`)
	_, err = callInspect(context.Background(), srv, Options{LibraryDir: filepath.Join(tmpDir, "lib")}, in)
	if err == nil {
		t.Fatal("expected error for nonexistent skill name")
	}

	in = json.RawMessage(`{}`)
	_, err = callInspect(context.Background(), srv, Options{LibraryDir: filepath.Join(tmpDir, "lib")}, in)
	if err == nil {
		t.Fatal("expected error when path and name are missing")
	}
	if !strings.Contains(err.Error(), "path or name") {
		t.Fatalf("expected 'path or name' error, got: %v", err)
	}
}

func TestSkillsInspectRequiresPathOrName(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	Register(srv, Options{LibraryDir: filepath.Join(tmpDir, "lib")})

	req := `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_inspect","arguments":{}}}` + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			IsError bool `json:"isError"`
		} `json:"result"`
		Error *struct {
			Code    int    `json:"code"`
			Message string `json:"message"`
		} `json:"error"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response %q: %v", out.String(), err)
	}

	if resp.Error == nil && !resp.Result.IsError {
		t.Fatal("expected error when path and name are missing")
	}
}

func TestSkillsRenderPlanRejectsMalformedArguments(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	Register(srv, Options{LibraryDir: filepath.Join(tmpDir, "lib"), RenderDir: filepath.Join(tmpDir, "render")})

	req := `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_render_plan","arguments":{"target":123}}}` + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			IsError bool `json:"isError"`
		} `json:"result"`
		Error *struct {
			Code    int    `json:"code"`
			Message string `json:"message"`
		} `json:"error"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response %q: %v", out.String(), err)
	}

	if resp.Error == nil && !resp.Result.IsError {
		t.Fatal("expected error for malformed arguments")
	}
}

func TestOptionsDefaults(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	opts := Options{}
	Register(srv, opts)

	req := `{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}` + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}
}

func TestRegisterBackfillsBaseDir(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	// Empty BaseDir should be backfilled from config.Defaults().
	Register(srv, Options{})

	// Verify the tools are registered (the backfill happens inside Register
	// and is captured by closures, so we just confirm no panic and tools
	// are available).
	req := `{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}` + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			Tools []struct {
				Name string `json:"name"`
			} `json:"tools"`
		} `json:"result"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse: %v", err)
	}
	if len(resp.Result.Tools) == 0 {
		t.Fatal("expected tools registered")
	}
}

func TestRegisterPreservesCustomBaseDir(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	customBase := "/custom/non/default/base"
	// A non-empty BaseDir should be preserved, not overwritten.
	Register(srv, Options{BaseDir: customBase, LibraryDir: t.TempDir()})

	// The tools should be registered and functional with the custom base dir.
	req := `{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}` + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}
}

func TestSkillsProfileList(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	profilesDir := filepath.Join(tmpDir, "profiles")
	if err := os.MkdirAll(profilesDir, 0755); err != nil {
		t.Fatalf("create profiles dir: %v", err)
	}
	if err := os.WriteFile(filepath.Join(profilesDir, "test.toml"), []byte("name = \"test-profile\"\ndescription = \"Test profile\"\n"), 0644); err != nil {
		t.Fatalf("write profile: %v", err)
	}

	Register(srv, Options{ProfilesDir: profilesDir})

	req := `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_profile_list","arguments":{}}}` + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			Content []struct {
				Type string          `json:"type"`
				Text json.RawMessage `json:"text"`
			} `json:"content"`
		} `json:"result"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response %q: %v", out.String(), err)
	}
	if len(resp.Result.Content) == 0 {
		t.Fatal("expected content in response")
	}

	var result struct {
		Profiles []struct {
			Name   string `json:"name"`
			Source string `json:"source"`
		} `json:"profiles"`
	}
	if err := parseText(resp.Result.Content[0].Text, &result); err != nil {
		t.Fatalf("parse result: %v", err)
	}

	if len(result.Profiles) != 1 {
		t.Fatalf("expected 1 profile, got %d", len(result.Profiles))
	}
	if result.Profiles[0].Name != "test" {
		t.Errorf("expected profile name 'test', got %q", result.Profiles[0].Name)
	}
	if result.Profiles[0].Source != "global" {
		t.Errorf("expected profile source 'global', got %q", result.Profiles[0].Source)
	}
}

func TestSkillsProfileResolve(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	libDir := filepath.Join(tmpDir, "lib")
	profilesDir := filepath.Join(tmpDir, "profiles")
	if err := os.MkdirAll(libDir, 0755); err != nil {
		t.Fatalf("create lib dir: %v", err)
	}
	if err := os.MkdirAll(profilesDir, 0755); err != nil {
		t.Fatalf("create profiles dir: %v", err)
	}

	skillDir := filepath.Join(libDir, "test-skill")
	if err := os.MkdirAll(skillDir, 0755); err != nil {
		t.Fatalf("create skill dir: %v", err)
	}
	writeTestSkill(t, skillDir)
	writeTestProfile(t, profilesDir, "test", "test-skill")

	Register(srv, Options{LibraryDir: libDir, ProfilesDir: profilesDir})

	req := `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_profile_resolve","arguments":{"name":"test"}}}` + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			Content []struct {
				Type string          `json:"type"`
				Text json.RawMessage `json:"text"`
			} `json:"content"`
		} `json:"result"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response %q: %v", out.String(), err)
	}
	if len(resp.Result.Content) == 0 {
		t.Fatal("expected content in response")
	}

	var result struct {
		Skills []struct {
			Name  string `json:"name"`
			Skill string `json:"skill"`
		} `json:"skills"`
		Issues []any `json:"issues"`
	}
	if err := parseText(resp.Result.Content[0].Text, &result); err != nil {
		t.Fatalf("parse result: %v", err)
	}

	if len(result.Skills) != 1 {
		t.Fatalf("expected 1 skill, got %d", len(result.Skills))
	}
	if result.Skills[0].Name != "test-skill" {
		t.Errorf("expected skill name 'test-skill', got %q", result.Skills[0].Name)
	}
	if result.Skills[0].Skill != "test-skill" {
		t.Errorf("expected skill 'test-skill', got %q", result.Skills[0].Skill)
	}
	if len(result.Issues) != 0 {
		t.Errorf("expected 0 issues, got %d", len(result.Issues))
	}
}

func TestSkillsRenderPlanProfile(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	libDir := filepath.Join(tmpDir, "lib")
	profilesDir := filepath.Join(tmpDir, "profiles")
	renderDir := filepath.Join(tmpDir, "render")
	if err := os.MkdirAll(libDir, 0755); err != nil {
		t.Fatalf("create lib dir: %v", err)
	}
	if err := os.MkdirAll(profilesDir, 0755); err != nil {
		t.Fatalf("create profiles dir: %v", err)
	}

	skillDir := filepath.Join(libDir, "test-skill")
	if err := os.MkdirAll(skillDir, 0755); err != nil {
		t.Fatalf("create skill dir: %v", err)
	}
	writeTestSkill(t, skillDir)
	writeTestProfile(t, profilesDir, "test", "test-skill")

	Register(srv, Options{LibraryDir: libDir, ProfilesDir: profilesDir, RenderDir: renderDir})

	req := `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_render_plan","arguments":{"profile":"test"}}}` + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			Content []struct {
				Type string          `json:"type"`
				Text json.RawMessage `json:"text"`
			} `json:"content"`
		} `json:"result"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response %q: %v", out.String(), err)
	}
	if len(resp.Result.Content) == 0 {
		t.Fatal("expected content in response")
	}

	var rendered []struct {
		Target string `json:"target"`
		Name   string `json:"name"`
	}
	if err := parseText(resp.Result.Content[0].Text, &rendered); err != nil {
		t.Fatalf("parse result: %v", err)
	}

	if len(rendered) != 6 {
		t.Fatalf("expected 6 rendered targets, got %d", len(rendered))
	}
	for _, item := range rendered {
		if item.Name != "test-skill" {
			t.Errorf("expected rendered name 'test-skill', got %q", item.Name)
		}
	}
}

func TestSkillsInstallProfileDryRun(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	libDir := filepath.Join(tmpDir, "lib")
	profilesDir := filepath.Join(tmpDir, "profiles")
	renderDir := filepath.Join(tmpDir, "render")
	if err := os.MkdirAll(libDir, 0755); err != nil {
		t.Fatalf("create lib dir: %v", err)
	}
	if err := os.MkdirAll(profilesDir, 0755); err != nil {
		t.Fatalf("create profiles dir: %v", err)
	}

	skillDir := filepath.Join(libDir, "test-skill")
	if err := os.MkdirAll(skillDir, 0755); err != nil {
		t.Fatalf("create skill dir: %v", err)
	}
	writeTestSkill(t, skillDir)
	writeTestProfile(t, profilesDir, "test", "test-skill")

	Register(srv, Options{LibraryDir: libDir, ProfilesDir: profilesDir, RenderDir: renderDir, HomeDir: tmpDir})

	req := `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_install","arguments":{"profile":"test","dry_run":true}}}` + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			Content []struct {
				Type string          `json:"type"`
				Text json.RawMessage `json:"text"`
			} `json:"content"`
		} `json:"result"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response %q: %v", out.String(), err)
	}
	if len(resp.Result.Content) == 0 {
		t.Fatal("expected content in response")
	}

	var results []struct {
		Action string `json:"action"`
		Name   string `json:"name"`
	}
	if err := parseText(resp.Result.Content[0].Text, &results); err != nil {
		t.Fatalf("parse result: %v", err)
	}

	if len(results) != 1 {
		t.Fatalf("expected 1 result, got %d", len(results))
	}
	if results[0].Action != "planned" {
		t.Errorf("expected action 'planned', got %q", results[0].Action)
	}
	if results[0].Name != "test-skill" {
		t.Errorf("expected name 'test-skill', got %q", results[0].Name)
	}
}

func TestSkillsRenderPlanProfileDryRun(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	libDir := filepath.Join(tmpDir, "lib")
	profilesDir := filepath.Join(tmpDir, "profiles")
	renderDir := filepath.Join(tmpDir, "render")
	if err := os.MkdirAll(libDir, 0755); err != nil {
		t.Fatalf("create lib dir: %v", err)
	}
	if err := os.MkdirAll(profilesDir, 0755); err != nil {
		t.Fatalf("create profiles dir: %v", err)
	}

	skillDir := filepath.Join(libDir, "test-skill")
	if err := os.MkdirAll(skillDir, 0755); err != nil {
		t.Fatalf("create skill dir: %v", err)
	}
	writeTestSkill(t, skillDir)
	writeTestProfile(t, profilesDir, "test", "test-skill")

	Register(srv, Options{LibraryDir: libDir, ProfilesDir: profilesDir, RenderDir: renderDir})

	req := `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_render_plan","arguments":{"profile":"test","dry_run":true}}}` + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			Content []struct {
				Type string          `json:"type"`
				Text json.RawMessage `json:"text"`
			} `json:"content"`
		} `json:"result"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response %q: %v", out.String(), err)
	}
	if len(resp.Result.Content) == 0 {
		t.Fatal("expected content in response")
	}

	var planned []struct {
		Target string `json:"target"`
		Name   string `json:"name"`
		Path   string `json:"path"`
	}
	if err := parseText(resp.Result.Content[0].Text, &planned); err != nil {
		t.Fatalf("parse result: %v", err)
	}

	if len(planned) != 6 {
		t.Fatalf("expected 6 planned targets, got %d", len(planned))
	}
	for _, item := range planned {
		if item.Name != "test-skill" {
			t.Errorf("expected planned name 'test-skill', got %q", item.Name)
		}
		if !strings.HasPrefix(item.Path, renderDir) {
			t.Errorf("expected planned path under render dir, got %q", item.Path)
		}
	}
	// Dry-run must not create the render directory.
	if _, err := os.Stat(renderDir); !os.IsNotExist(err) {
		t.Errorf("dry-run created render dir at %s (err=%v)", renderDir, err)
	}
}

func TestSkillsInstallProfileWrite(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	libDir := filepath.Join(tmpDir, "lib")
	profilesDir := filepath.Join(tmpDir, "profiles")
	renderDir := filepath.Join(tmpDir, "render")
	if err := os.MkdirAll(libDir, 0755); err != nil {
		t.Fatalf("create lib dir: %v", err)
	}
	if err := os.MkdirAll(profilesDir, 0755); err != nil {
		t.Fatalf("create profiles dir: %v", err)
	}

	skillDir := filepath.Join(libDir, "test-skill")
	if err := os.MkdirAll(skillDir, 0755); err != nil {
		t.Fatalf("create skill dir: %v", err)
	}
	writeTestSkill(t, skillDir)
	writeTestProfile(t, profilesDir, "test", "test-skill")

	Register(srv, Options{LibraryDir: libDir, ProfilesDir: profilesDir, RenderDir: renderDir, HomeDir: tmpDir})

	req := `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_install","arguments":{"profile":"test","dry_run":false,"target":"opencode"}}}` + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			Content []struct {
				Type string          `json:"type"`
				Text json.RawMessage `json:"text"`
			} `json:"content"`
		} `json:"result"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response %q: %v", out.String(), err)
	}
	if len(resp.Result.Content) == 0 {
		t.Fatal("expected content in response")
	}

	var results []struct {
		Action string `json:"action"`
		Name   string `json:"name"`
		Path   string `json:"path"`
	}
	if err := parseText(resp.Result.Content[0].Text, &results); err != nil {
		t.Fatalf("parse result: %v", err)
	}

	if len(results) != 1 {
		t.Fatalf("expected 1 result, got %d", len(results))
	}
	if results[0].Action != "installed" {
		t.Errorf("expected action 'installed', got %q", results[0].Action)
	}
	if results[0].Name != "test-skill" {
		t.Errorf("expected name 'test-skill', got %q", results[0].Name)
	}
	// The write path must have produced an actual install (marker file).
	marker := filepath.Join(results[0].Path, ".symskills.json")
	if _, err := os.Stat(marker); err != nil {
		t.Errorf("expected install marker at %s: %v", marker, err)
	}
}

func TestSkillsRenderPlanProfileMissingSkill(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	libDir := filepath.Join(tmpDir, "lib")
	profilesDir := filepath.Join(tmpDir, "profiles")
	renderDir := filepath.Join(tmpDir, "render")
	if err := os.MkdirAll(libDir, 0755); err != nil {
		t.Fatalf("create lib dir: %v", err)
	}
	if err := os.MkdirAll(profilesDir, 0755); err != nil {
		t.Fatalf("create profiles dir: %v", err)
	}

	writeTestProfile(t, profilesDir, "test", "missing-skill")

	Register(srv, Options{LibraryDir: libDir, ProfilesDir: profilesDir, RenderDir: renderDir})

	req := `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_render_plan","arguments":{"profile":"test"}}}` + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			Content []struct {
				Type string          `json:"type"`
				Text json.RawMessage `json:"text"`
			} `json:"content"`
		} `json:"result"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response %q: %v", out.String(), err)
	}
	if len(resp.Result.Content) == 0 {
		t.Fatal("expected content in response")
	}

	var result struct {
		Skills []any `json:"skills"`
		Issues []any `json:"issues"`
	}
	if err := parseText(resp.Result.Content[0].Text, &result); err != nil {
		t.Fatalf("parse result: %v", err)
	}
	if len(result.Skills) != 0 {
		t.Errorf("expected 0 skills, got %d", len(result.Skills))
	}
	if len(result.Issues) == 0 {
		t.Errorf("expected issues for missing skill, got none")
	}
}

func TestSkillsListNonEmptyLibrary(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	libDir := filepath.Join(tmpDir, "lib")
	if err := os.MkdirAll(libDir, 0755); err != nil {
		t.Fatalf("create lib dir: %v", err)
	}
	skillDir := filepath.Join(libDir, "test-skill")
	if err := os.MkdirAll(skillDir, 0755); err != nil {
		t.Fatalf("create skill dir: %v", err)
	}
	writeTestSkill(t, skillDir)

	Register(srv, Options{LibraryDir: libDir})

	req := `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_list","arguments":{}}}` + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			Content []struct {
				Type string          `json:"type"`
				Text json.RawMessage `json:"text"`
			} `json:"content"`
		} `json:"result"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response %q: %v", out.String(), err)
	}
	if len(resp.Result.Content) == 0 {
		t.Fatal("expected content in response")
	}

	var result struct {
		Skills []struct {
			Name        string `json:"name"`
			Description string `json:"description"`
		} `json:"skills"`
		Issues []any `json:"issues"`
	}
	if err := parseText(resp.Result.Content[0].Text, &result); err != nil {
		t.Fatalf("parse result: %v", err)
	}
	if len(result.Skills) != 1 {
		t.Fatalf("expected 1 skill, got %d", len(result.Skills))
	}
	if result.Skills[0].Name != "test-skill" {
		t.Errorf("expected name 'test-skill', got %q", result.Skills[0].Name)
	}
}

func TestSkillsValidateWithIssues(t *testing.T) {
	// A skill with name>64 chars triggers a validation issue.
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	libDir := filepath.Join(tmpDir, "lib")
	if err := os.MkdirAll(libDir, 0755); err != nil {
		t.Fatalf("create lib dir: %v", err)
	}
	skillDir := filepath.Join(libDir, "bad-skill")
	if err := os.MkdirAll(skillDir, 0755); err != nil {
		t.Fatalf("create skill dir: %v", err)
	}
	content := "---\nname: " + strings.Repeat("x", 65) + "\ndescription: invalid\n---\n# Body\n"
	if err := os.WriteFile(filepath.Join(skillDir, "SKILL.md"), []byte(content), 0644); err != nil {
		t.Fatalf("write SKILL.md: %v", err)
	}

	Register(srv, Options{LibraryDir: libDir})

	req := fmt.Sprintf(`{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_validate","arguments":{"path":"%s"}}}`, skillDir) + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			Content []struct {
				Type string          `json:"type"`
				Text json.RawMessage `json:"text"`
			} `json:"content"`
		} `json:"result"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response %q: %v", out.String(), err)
	}
	if len(resp.Result.Content) == 0 {
		t.Fatal("expected content in response")
	}

	var result struct {
		Issues []any `json:"issues"`
	}
	if err := parseText(resp.Result.Content[0].Text, &result); err != nil {
		t.Fatalf("parse result: %v", err)
	}
	if len(result.Issues) == 0 {
		t.Fatal("expected validation issues for bad skill name, got none")
	}
}

func TestSkillsRenderPlanInvalidTarget(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	libDir := filepath.Join(tmpDir, "lib")
	renderDir := filepath.Join(tmpDir, "render")
	if err := os.MkdirAll(libDir, 0755); err != nil {
		t.Fatalf("create lib dir: %v", err)
	}
	skillDir := filepath.Join(libDir, "test-skill")
	if err := os.MkdirAll(skillDir, 0755); err != nil {
		t.Fatalf("create skill dir: %v", err)
	}
	writeTestSkill(t, skillDir)

	Register(srv, Options{LibraryDir: libDir, RenderDir: renderDir})

	req := fmt.Sprintf(`{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_render_plan","arguments":{"path":"%s","target":"invalid"}}}`, skillDir) + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			IsError bool `json:"isError"`
		} `json:"result"`
		Error *struct {
			Code    int    `json:"code"`
			Message string `json:"message"`
		} `json:"error"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response %q: %v", out.String(), err)
	}
	if resp.Error == nil && !resp.Result.IsError {
		t.Fatal("expected error for invalid target")
	}
}

func TestSkillsRenderPlanSingleSkill(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	libDir := filepath.Join(tmpDir, "lib")
	renderDir := filepath.Join(tmpDir, "render")
	if err := os.MkdirAll(libDir, 0755); err != nil {
		t.Fatalf("create lib dir: %v", err)
	}
	skillDir := filepath.Join(libDir, "test-skill")
	if err := os.MkdirAll(skillDir, 0755); err != nil {
		t.Fatalf("create skill dir: %v", err)
	}
	writeTestSkill(t, skillDir)

	Register(srv, Options{LibraryDir: libDir, RenderDir: renderDir, HomeDir: tmpDir})

	req := fmt.Sprintf(`{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_render_plan","arguments":{"path":"%s"}}}`, skillDir) + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			Content []struct {
				Type string          `json:"type"`
				Text json.RawMessage `json:"text"`
			} `json:"content"`
		} `json:"result"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response %q: %v", out.String(), err)
	}
	if len(resp.Result.Content) == 0 {
		t.Fatal("expected content in response")
	}
}

func TestSkillsInstallSingleSkillDryRun(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	libDir := filepath.Join(tmpDir, "lib")
	renderDir := filepath.Join(tmpDir, "render")
	if err := os.MkdirAll(libDir, 0755); err != nil {
		t.Fatalf("create lib dir: %v", err)
	}
	skillDir := filepath.Join(libDir, "test-skill")
	if err := os.MkdirAll(skillDir, 0755); err != nil {
		t.Fatalf("create skill dir: %v", err)
	}
	writeTestSkill(t, skillDir)

	Register(srv, Options{LibraryDir: libDir, RenderDir: renderDir, HomeDir: tmpDir})

	req := fmt.Sprintf(`{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_install","arguments":{"path":"%s","dry_run":true}}}`, skillDir) + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			Content []struct {
				Type string          `json:"type"`
				Text json.RawMessage `json:"text"`
			} `json:"content"`
		} `json:"result"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response %q: %v", out.String(), err)
	}
	if len(resp.Result.Content) == 0 {
		t.Fatal("expected content in response")
	}

	var result struct {
		Action string `json:"action"`
		Name   string `json:"name"`
	}
	if err := parseText(resp.Result.Content[0].Text, &result); err != nil {
		t.Fatalf("parse result: %v", err)
	}
	if result.Action != "planned" {
		t.Errorf("expected action 'planned', got %q", result.Action)
	}
	if result.Name != "test-skill" {
		t.Errorf("expected name 'test-skill', got %q", result.Name)
	}
}

func TestSkillsInstallMissingPathOrName(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	Register(srv, Options{
		LibraryDir: filepath.Join(tmpDir, "lib"),
		RenderDir:  filepath.Join(tmpDir, "render"),
		HomeDir:    tmpDir,
	})

	req := `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_install","arguments":{}}}` + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			IsError bool `json:"isError"`
		} `json:"result"`
		Error *struct {
			Code    int    `json:"code"`
			Message string `json:"message"`
		} `json:"error"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response %q: %v", out.String(), err)
	}
	if resp.Error == nil && !resp.Result.IsError {
		t.Fatal("expected error when path and name are missing")
	}
}

func TestSkillsInstallProfileMissingSkill(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	libDir := filepath.Join(tmpDir, "lib")
	profilesDir := filepath.Join(tmpDir, "profiles")
	renderDir := filepath.Join(tmpDir, "render")
	if err := os.MkdirAll(libDir, 0755); err != nil {
		t.Fatalf("create lib dir: %v", err)
	}
	if err := os.MkdirAll(profilesDir, 0755); err != nil {
		t.Fatalf("create profiles dir: %v", err)
	}

	writeTestProfile(t, profilesDir, "test", "missing-skill")

	Register(srv, Options{LibraryDir: libDir, ProfilesDir: profilesDir, RenderDir: renderDir, HomeDir: tmpDir})

	req := `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_install","arguments":{"profile":"test","dry_run":true}}}` + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			Content []struct {
				Type string          `json:"type"`
				Text json.RawMessage `json:"text"`
			} `json:"content"`
		} `json:"result"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response %q: %v", out.String(), err)
	}
	if len(resp.Result.Content) == 0 {
		t.Fatal("expected content in response")
	}

	var result struct {
		Results []any `json:"results"`
		Issues  []any `json:"issues"`
	}
	if err := parseText(resp.Result.Content[0].Text, &result); err != nil {
		t.Fatalf("parse result: %v", err)
	}
	if len(result.Results) != 0 {
		t.Errorf("expected 0 results, got %d", len(result.Results))
	}
	if len(result.Issues) == 0 {
		t.Errorf("expected issues for missing skill, got none")
	}
}

// TestMCPTextIsJSONString verifies that tool results are serialized
// as JSON strings (TextContent.text is a string, not a raw object).
func TestMCPTextIsJSONString(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	libDir := filepath.Join(tmpDir, "lib")
	if err := os.MkdirAll(libDir, 0755); err != nil {
		t.Fatalf("create lib dir: %v", err)
	}
	skillDir := filepath.Join(libDir, "test-skill")
	if err := os.MkdirAll(skillDir, 0755); err != nil {
		t.Fatalf("create skill dir: %v", err)
	}
	writeTestSkill(t, skillDir)

	Register(srv, Options{LibraryDir: libDir})

	req := `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_list","arguments":{}}}` + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	// Verify the text field is a JSON string (wrap response in object for valid JSON)
	type contentItem struct {
		Type string          `json:"type"`
		Text json.RawMessage `json:"text"`
	}
	type resultBlock struct {
		Content []contentItem `json:"content"`
	}
	type response struct {
		Result resultBlock `json:"result"`
	}
	var resp response
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response %q: %v", out.String(), err)
	}
	if len(resp.Result.Content) == 0 {
		t.Fatal("expected content in response")
	}

	// Text must be a JSON string (starts with '"')
	text := resp.Result.Content[0].Text
	if len(text) == 0 || text[0] != '"' {
		t.Fatalf("expected Text to be a JSON string, got: %s", string(text))
	}
	if text[len(text)-1] != '"' {
		t.Fatalf("expected Text to be a JSON string ending with '\"', got: %s", string(text))
	}
}

// TestSkillsRenderPlanDryRunDoesNotWrite verifies that dry_run=true on
// skills_render_plan does not write files to the render directory.
func TestSkillsRenderPlanDryRunDoesNotWrite(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	libDir := filepath.Join(tmpDir, "lib")
	renderDir := filepath.Join(tmpDir, "render")
	if err := os.MkdirAll(libDir, 0755); err != nil {
		t.Fatalf("create lib dir: %v", err)
	}
	skillDir := filepath.Join(libDir, "test-skill")
	if err := os.MkdirAll(skillDir, 0755); err != nil {
		t.Fatalf("create skill dir: %v", err)
	}
	writeTestSkill(t, skillDir)

	Register(srv, Options{LibraryDir: libDir, RenderDir: renderDir})

	req := fmt.Sprintf(`{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_render_plan","arguments":{"path":"%s","dry_run":true}}}`, skillDir) + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			Content []struct {
				Type string          `json:"type"`
				Text json.RawMessage `json:"text"`
			} `json:"content"`
		} `json:"result"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response: %v", err)
	}
	if len(resp.Result.Content) == 0 {
		t.Fatal("expected content in response")
	}

	// Verify the render directory has no written files
	entries, err := os.ReadDir(renderDir)
	if err != nil && !os.IsNotExist(err) {
		t.Fatalf("read render dir: %v", err)
	}
	if len(entries) > 0 {
		t.Errorf("expected empty render dir after dry_run, got %d entries", len(entries))
	}
}

// TestSkillsInstallDryRunDoesNotWriteRenderDir verifies that dry_run=true
// on skills_install does not write to the render directory.
func TestSkillsInstallDryRunDoesNotWriteRenderDir(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	libDir := filepath.Join(tmpDir, "lib")
	renderDir := filepath.Join(tmpDir, "render")
	if err := os.MkdirAll(libDir, 0755); err != nil {
		t.Fatalf("create lib dir: %v", err)
	}
	skillDir := filepath.Join(libDir, "test-skill")
	if err := os.MkdirAll(skillDir, 0755); err != nil {
		t.Fatalf("create skill dir: %v", err)
	}
	writeTestSkill(t, skillDir)

	Register(srv, Options{LibraryDir: libDir, RenderDir: renderDir, HomeDir: tmpDir})

	req := fmt.Sprintf(`{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_install","arguments":{"path":"%s","dry_run":true}}}`, skillDir) + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			Content []struct {
				Type string          `json:"type"`
				Text json.RawMessage `json:"text"`
			} `json:"content"`
		} `json:"result"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response: %v", err)
	}
	if len(resp.Result.Content) == 0 {
		t.Fatal("expected content in response")
	}

	// Verify the render directory has no written files
	entries, err := os.ReadDir(renderDir)
	if err != nil && !os.IsNotExist(err) {
		t.Fatalf("read render dir: %v", err)
	}
	if len(entries) > 0 {
		t.Errorf("expected empty render dir after dry_run, got %d entries", len(entries))
	}
}

func TestSkillsTargetsStatusMCP(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	Register(srv, Options{HomeDir: tmpDir})

	req := `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_targets_status","arguments":{}}}` + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			Content []struct {
				Type string          `json:"type"`
				Text json.RawMessage `json:"text"`
			} `json:"content"`
		} `json:"result"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response: %v", err)
	}
	if len(resp.Result.Content) == 0 {
		t.Fatal("expected content in response")
	}

	var data struct {
		Targets []struct {
			Target string `json:"target"`
		} `json:"targets"`
	}
	if err := parseText(resp.Result.Content[0].Text, &data); err != nil {
		t.Fatalf("parse text: %v", err)
	}
	if len(data.Targets) != 6 {
		t.Fatalf("expected 6 target statuses, got %d", len(data.Targets))
	}
}

func TestSkillsDiscoverSourcesMCP(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	skillDir := filepath.Join(tmpDir, "disc-skill")
	if err := os.MkdirAll(skillDir, 0755); err != nil {
		t.Fatalf("mkdir: %v", err)
	}
	writeTestSkill(t, skillDir)

	Register(srv, Options{HomeDir: tmpDir})

	req := fmt.Sprintf(`{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_discover_sources","arguments":{"paths":["%s"]}}}`, skillDir) + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			Content []struct {
				Type string          `json:"type"`
				Text json.RawMessage `json:"text"`
			} `json:"content"`
		} `json:"result"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response: %v", err)
	}
	if len(resp.Result.Content) == 0 {
		t.Fatal("expected content in response")
	}

	var data struct {
		Candidates []struct {
			SourceID string `json:"source_id"`
			Location string `json:"location"`
		} `json:"candidates"`
	}
	if err := parseText(resp.Result.Content[0].Text, &data); err != nil {
		t.Fatalf("parse text: %v", err)
	}
	if len(data.Candidates) != 1 {
		t.Fatalf("expected 1 candidate, got %d", len(data.Candidates))
	}
}

func TestSkillsListIncludesMetadata(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	libDir := filepath.Join(tmpDir, "lib")
	if err := os.MkdirAll(libDir, 0755); err != nil {
		t.Fatalf("create lib dir: %v", err)
	}
	skillDir := filepath.Join(libDir, "test-skill")
	if err := os.MkdirAll(skillDir, 0755); err != nil {
		t.Fatalf("create skill dir: %v", err)
	}
	writeTestSkill(t, skillDir)

	// An install event in the lifecycle log supplies the install record.
	logPath := filepath.Join(tmpDir, "events.jsonl")
	logger := events.New(logPath, "test")
	logger.Record(events.Event{Event: events.EventInstall, Skill: "test-skill", Target: "opencode", Path: filepath.Join(tmpDir, "skills", "test-skill"), Outcome: events.OutcomeOK})

	Register(srv, Options{LibraryDir: libDir, HomeDir: tmpDir, EventsPath: logPath})

	req := `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_list","arguments":{}}}` + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			Content []struct {
				Type string          `json:"type"`
				Text json.RawMessage `json:"text"`
			} `json:"content"`
		} `json:"result"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response %q: %v", out.String(), err)
	}
	var result struct {
		Skills []struct {
			Name           string `json:"name"`
			CreatedAt      string `json:"created_at"`
			ModifiedAt     string `json:"modified_at"`
			LastRenderedAt string `json:"last_rendered_at"`
			Installs       []struct {
				Target      string `json:"target"`
				Path        string `json:"path"`
				InstalledAt string `json:"installed_at"`
			} `json:"installs"`
			LastUsed       *string `json:"last_used"`
			LastUsedSource string  `json:"last_used_source"`
		} `json:"skills"`
	}
	if err := parseText(resp.Result.Content[0].Text, &result); err != nil {
		t.Fatalf("parse result: %v", err)
	}
	if len(result.Skills) != 1 {
		t.Fatalf("expected 1 skill, got %d", len(result.Skills))
	}
	skill := result.Skills[0]
	if skill.Name != "test-skill" {
		t.Errorf("expected test-skill, got %q", skill.Name)
	}
	if skill.CreatedAt == "" || skill.ModifiedAt == "" {
		t.Errorf("expected created_at and modified_at, got %q / %q", skill.CreatedAt, skill.ModifiedAt)
	}
	if len(skill.Installs) != 1 {
		t.Fatalf("expected 1 install from the event log, got %v", skill.Installs)
	}
	if skill.Installs[0].Target != "opencode" {
		t.Errorf("expected opencode install, got %+v", skill.Installs[0])
	}
	if skill.Installs[0].InstalledAt == "" {
		t.Error("expected installed_at from the event log")
	}
	if skill.LastUsed != nil {
		t.Errorf("expected null last_used, got %q", *skill.LastUsed)
	}
	if skill.LastUsedSource != "" {
		t.Errorf("expected empty last_used_source, got %q", skill.LastUsedSource)
	}
}

func TestSkillsInspectIncludesMetadata(t *testing.T) {
	srv := mcpserver.New("symskills", "test")
	tmpDir := t.TempDir()
	libDir := filepath.Join(tmpDir, "lib")
	if err := os.MkdirAll(libDir, 0755); err != nil {
		t.Fatalf("create lib dir: %v", err)
	}
	skillDir := filepath.Join(libDir, "test-skill")
	if err := os.MkdirAll(skillDir, 0755); err != nil {
		t.Fatalf("create skill dir: %v", err)
	}
	writeTestSkill(t, skillDir)

	Register(srv, Options{LibraryDir: libDir, HomeDir: tmpDir})

	req := `{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"skills_inspect","arguments":{"name":"test-skill"}}}` + "\n"
	var out strings.Builder
	if err := srv.ServeIO(context.Background(), strings.NewReader(req), &out); err != nil {
		t.Fatalf("ServeIO: %v", err)
	}

	var resp struct {
		Result struct {
			Content []struct {
				Type string          `json:"type"`
				Text json.RawMessage `json:"text"`
			} `json:"content"`
		} `json:"result"`
	}
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("parse response %q: %v", out.String(), err)
	}
	var result struct {
		Root       string  `json:"root"`
		CreatedAt  string  `json:"created_at"`
		ModifiedAt string  `json:"modified_at"`
		Installs   []any   `json:"installs"`
		LastUsed   *string `json:"last_used"`
	}
	if err := parseText(resp.Result.Content[0].Text, &result); err != nil {
		t.Fatalf("parse result: %v", err)
	}
	if result.Root != skillDir {
		t.Errorf("expected root %q, got %q", skillDir, result.Root)
	}
	if result.CreatedAt == "" || result.ModifiedAt == "" {
		t.Errorf("expected created_at and modified_at, got %q / %q", result.CreatedAt, result.ModifiedAt)
	}
	if result.Installs == nil {
		t.Error("expected installs to be an array, got null")
	}
	if result.LastUsed != nil {
		t.Errorf("expected null last_used, got %q", *result.LastUsed)
	}
}
