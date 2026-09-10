package gateway

import (
	"bytes"
	"context"
	"log/slog"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/broker"
	"github.com/danieljustus/symaira-brain/internal/policy"
	"github.com/danieljustus/symaira-brain/internal/profile"
)

// --- helpers ---

func newManagedFake(t *testing.T, name string, toolsJSON string) *broker.ManagedServer {
	t.Helper()
	ms := broker.NewManagedServer(broker.ServerConfig{
		Name:        name,
		BinaryPath:  fakeBin,
		MaxRestarts: 0,
		Env:         []string{"FAKEMCP_TOOLS=" + toolsJSON},
	})
	t.Cleanup(func() { ms.Shutdown() })

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	_, err := ms.ListTools(ctx)
	if err != nil {
		t.Fatalf("ListTools(%s): %v", name, err)
	}
	return ms
}

func testProfile() *profile.Profile {
	return &profile.Profile{
		Name: "test",
		Servers: profile.Servers{
			"vault":  profile.ServerConfig{Enabled: true, Mode: profile.VaultModeFull},
			"memory": profile.ServerConfig{Enabled: true, Mode: profile.MemoryModeReadWrite},
			"skills": profile.ServerConfig{Enabled: true},
		},
		Audit: profile.AuditConfig{Enabled: false},
	}
}

// --- New() tests ---

func TestNew_NilLogger(t *testing.T) {
	s := New(testProfile(), nil, nil, nil, "dev")
	if s == nil {
		t.Fatal("New returned nil")
	}
	if s.logger == nil {
		t.Error("logger should default to slog.Default()")
	}
}

func TestNew_WithLogger(t *testing.T) {
	logger := slog.New(slog.NewTextHandler(bytes.NewBuffer(nil), nil))
	s := New(testProfile(), nil, logger, nil, "dev")
	if s.logger != logger {
		t.Error("logger should be the one provided")
	}
}

// --- buildCatalog() tests ---

func TestBuildCatalog_MergesToolsFromMultipleServers(t *testing.T) {
	vault := newManagedFake(t, "vault",
		`[{"name":"get_entry","description":"fetch secret"},{"name":"health","description":"healthcheck"}]`)
	memory := newManagedFake(t, "memory",
		`[{"name":"memory_search","description":"search memories"},{"name":"entity_list","description":"list entities"}]`)

	servers := map[string]*broker.ManagedServer{
		"vault":  vault,
		"memory": memory,
	}

	s := New(testProfile(), servers, slog.Default(), nil, "dev")

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	if err := s.buildCatalog(ctx); err != nil {
		t.Fatalf("buildCatalog: %v", err)
	}
	if s.cat == nil {
		t.Fatal("catalog should be set after buildCatalog")
	}

	exposed := s.cat.Exposed()
	names := make(map[string]bool)
	for _, e := range exposed {
		names[e.Name] = true
	}

	for _, want := range []string{"vault_get_entry", "vault_health", "memory_search", "entity_list"} {
		if !names[want] {
			t.Errorf("missing exposed tool %q", want)
		}
	}
}

func TestBuildCatalog_ForeignServerReadAccess(t *testing.T) {
	// A foreign server (beyond the four cores) is a filter, not a
	// gatekeeper: access="read" exposes only tools classified as reading.
	// readOnlyHint drives the classification, the resolved class + source
	// land on the catalog entry for the audit log.
	p := &profile.Profile{
		Name: "test",
		Servers: profile.Servers{
			"fig": profile.ServerConfig{Enabled: true, Command: "/bin/true", Access: profile.ForeignAccessRead},
		},
	}
	fig := newManagedFake(t, "fig", `[
		{"name":"search","description":"read-only search","annotations":{"readOnlyHint":true}},
		{"name":"delete","description":"destructive","annotations":{"readOnlyHint":false}},
		{"name":"blob","description":"no hint"}
	]`)
	servers := map[string]*broker.ManagedServer{"fig": fig}

	s := New(p, servers, slog.Default(), nil, "dev")

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	if err := s.buildCatalog(ctx); err != nil {
		t.Fatalf("buildCatalog: %v", err)
	}

	exposed := s.cat.Exposed()
	for _, e := range exposed {
		if e.OriginalName != "search" {
			t.Errorf("foreign server exposed tool %q (orig %q), want only search", e.Name, e.OriginalName)
		}
	}
	if len(exposed) != 1 {
		t.Fatalf("Exposed() = %d entries, want 1 (fig_search)", len(exposed))
	}

	entry := exposed[0]
	if entry.AccessClass != "read" || entry.AccessSource != policy.ExposureSourceReadOnlyHint {
		t.Errorf("search access = %s/%s, want read/read_only_hint", entry.AccessClass, entry.AccessSource)
	}
}

func TestBuildCatalog_ForeignServerExplicitToolsReadWins(t *testing.T) {
	// An explicit tools_read entry overrides the upstream hint: a tool the
	// server marks write-only is reclassified read and survives access="read".
	p := &profile.Profile{
		Name: "test",
		Servers: profile.Servers{
			"fig": profile.ServerConfig{
				Enabled:    true,
				Command:    "/bin/true",
				Access:     profile.ForeignAccessRead,
				ToolsRead:  []string{"upsert"},
				ToolsWrite: []string{"search"}, // overrides readOnlyHint=true
			},
		},
	}
	fig := newManagedFake(t, "fig", `[
		{"name":"upsert","description":"marked write by server","annotations":{"readOnlyHint":false}},
		{"name":"search","description":"marked read by server","annotations":{"readOnlyHint":true}}
	]`)
	servers := map[string]*broker.ManagedServer{"fig": fig}

	s := New(p, servers, slog.Default(), nil, "dev")

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	if err := s.buildCatalog(ctx); err != nil {
		t.Fatalf("buildCatalog: %v", err)
	}

	exposed := s.cat.Exposed()
	if len(exposed) != 1 || exposed[0].OriginalName != "upsert" {
		t.Fatalf("Exposed() = %+v, want only upsert (tools_read reclassifies, tools_write hides)", exposed)
	}
	if exposed[0].AccessSource != policy.ExposureSourceToolsRead {
		t.Errorf("upsert access source = %q, want tools_read", exposed[0].AccessSource)
	}
}

func TestBuildCatalog_ForeignServerReadAccessYieldingNothingDegrades(t *testing.T) {
	// Most upstream servers don't set readOnlyHint, so every tool falls
	// through to default_write and access=read hides all of them. That
	// must not look like a silently broken profile — it should surface as
	// a degradation naming the server and explaining why (issue #444).
	p := &profile.Profile{
		Name: "test",
		Servers: profile.Servers{
			"fig": profile.ServerConfig{Enabled: true, Command: "/bin/true", Access: profile.ForeignAccessRead},
		},
	}
	fig := newManagedFake(t, "fig", `[
		{"name":"write_doc","description":"no hint"},
		{"name":"delete_doc","description":"no hint either"}
	]`)
	servers := map[string]*broker.ManagedServer{"fig": fig}

	s := New(p, servers, slog.Default(), nil, "dev")

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	if err := s.buildCatalog(ctx); err != nil {
		t.Fatalf("buildCatalog: %v", err)
	}

	if len(s.cat.Exposed()) != 0 {
		t.Fatalf("Exposed() = %+v, want none (both tools default_write, access=read hides both)", s.cat.Exposed())
	}

	if len(s.degradations) != 1 {
		t.Fatalf("degradations = %+v, want exactly 1 explaining the empty exposure", s.degradations)
	}
	d := s.degradations[0]
	if d.Server != "fig" {
		t.Errorf("degradation.Server = %q, want fig", d.Server)
	}
	if !strings.Contains(d.Reason, "0 of 2 tools") || !strings.Contains(d.Reason, "tools_read") {
		t.Errorf("degradation.Reason = %q, want it to name the tool count and mention tools_read", d.Reason)
	}
}

func TestBuildCatalog_ForeignServerReadAccessWithSomeExposureDoesNotDegrade(t *testing.T) {
	// The degradation is specifically for a fully empty exposure — a
	// partial one (some tools read, some hidden) is exactly the filter
	// model working as intended and must not warn.
	p := &profile.Profile{
		Name: "test",
		Servers: profile.Servers{
			"fig": profile.ServerConfig{Enabled: true, Command: "/bin/true", Access: profile.ForeignAccessRead},
		},
	}
	fig := newManagedFake(t, "fig", `[
		{"name":"search","description":"read-only","annotations":{"readOnlyHint":true}},
		{"name":"write_doc","description":"no hint"}
	]`)
	servers := map[string]*broker.ManagedServer{"fig": fig}

	s := New(p, servers, slog.Default(), nil, "dev")

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	if err := s.buildCatalog(ctx); err != nil {
		t.Fatalf("buildCatalog: %v", err)
	}

	if len(s.cat.Exposed()) != 1 {
		t.Fatalf("Exposed() = %+v, want exactly 1 (search)", s.cat.Exposed())
	}
	if len(s.degradations) != 0 {
		t.Errorf("degradations = %+v, want none (partial exposure is the filter model working as intended)", s.degradations)
	}
}

func TestBuildCatalog_SkipsDisabledServers(t *testing.T) {
	vault := newManagedFake(t, "vault",
		`[{"name":"get_entry","description":"fetch secret"}]`)

	p := testProfile()
	p.Servers["memory"] = profile.ServerConfig{Enabled: false}

	servers := map[string]*broker.ManagedServer{
		"vault":  vault,
		"memory": newManagedFake(t, "memory", `[]`),
	}

	s := New(p, servers, slog.Default(), nil, "dev")

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	if err := s.buildCatalog(ctx); err != nil {
		t.Fatalf("buildCatalog: %v", err)
	}

	exposed := s.cat.Exposed()
	for _, e := range exposed {
		if e.Server == "memory" {
			t.Errorf("memory tool %q should not be exposed when memory is disabled", e.Name)
		}
	}
}

func TestBuildCatalog_PolicyFiltering(t *testing.T) {
	vault := newManagedFake(t, "vault",
		`[{"name":"get_entry","description":"fetch secret"},{"name":"health","description":"hc"},{"name":"request_credential","description":"req"}]`)

	p := testProfile()
	vaultCfg := p.Servers["vault"]
	vaultCfg.Mode = profile.VaultModeRequestOnly
	p.Servers["vault"] = vaultCfg

	servers := map[string]*broker.ManagedServer{
		"vault": vault,
	}

	s := New(p, servers, slog.Default(), nil, "dev")

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	if err := s.buildCatalog(ctx); err != nil {
		t.Fatalf("buildCatalog: %v", err)
	}

	exposed := s.cat.Exposed()
	exposedNames := make(map[string]bool)
	for _, e := range exposed {
		exposedNames[e.Name] = true
	}

	if exposedNames["vault_get_entry"] {
		t.Error("vault_get_entry should be hidden in request_only mode")
	}
	if !exposedNames["vault_health"] {
		t.Error("vault_health should be exposed in request_only mode")
	}
	if !exposedNames["vault_request_credential"] {
		t.Error("vault_request_credential should be exposed in request_only mode")
	}
}

func TestBuildCatalog_EmptyServers(t *testing.T) {
	s := New(testProfile(), map[string]*broker.ManagedServer{}, slog.Default(), nil, "dev")

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	if err := s.buildCatalog(ctx); err != nil {
		t.Fatalf("buildCatalog with empty servers: %v", err)
	}

	exposed := s.cat.Exposed()
	if len(exposed) != 0 {
		t.Errorf("expected 0 exposed tools, got %d", len(exposed))
	}
}

// --- routeToolCall() tests ---
