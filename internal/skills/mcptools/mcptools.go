// Package mcptools exposes symskills workflows over MCP.
package mcptools

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/signal"
	"syscall"

	"github.com/danieljustus/symaira-brain/internal/skills/metadata"
	"github.com/danieljustus/symaira-corekit/mcpserver"
)

const emptyObject = `{"type":"object","properties":{}}`

type Options struct {
	LibraryDir  string
	RenderDir   string
	CacheDir    string
	BaseDir     string
	ProfilesDir string
	HomeDir     string
	ProjectDir  string
	// Version is the symskills build version stamped into event log
	// records as tool_version.
	Version string
	// EventsPath overrides the operation-log location. When empty, the log
	// lives under HomeDir (or the default home when HomeDir is empty too).
	EventsPath string
	// VCSEnabled mirrors the vcs.enabled config toggle: skills_restore
	// refuses to write to the per-skill repositories while it is false.
	// Nil means enabled (the default), so bare Options{} in tests keeps
	// working.
	VCSEnabled *bool
}

// skillListItem is one skills_list row: the frontmatter summary plus the
// per-skill metadata record (same snake_case fields as `symskills list
// --json`).
type skillListItem struct {
	Name        string `json:"name"`
	Description string `json:"description"`
	Category    string `json:"category,omitempty"`
	Root        string `json:"root"`
	metadata.Record
}

// mcpJSON marshals v to JSON and returns it as a string suitable for
// TextContent.text. The MCP protocol requires the text field to be a
// JSON string, not a raw JSON object; this helper ensures structured
// results are always serialized to a JSON string before the mcpserver
// wire layer embeds them.
func mcpJSON(v any) (string, error) {
	data, err := json.Marshal(v)
	if err != nil {
		return "", fmt.Errorf("serialize result: %w", err)
	}
	return string(data), nil
}

// Register registers all symskills MCP tools onto srv. It is a thin
// dispatcher: shared state (backfilled Options, logger, log path) is
// computed once and handed to per-group registration functions that live
// in separate files — mcptools_library.go, mcptools_render.go,
// mcptools_profiles.go, mcptools_versioning.go, mcptools_status.go — so
// this file holds no tool bodies.
func Register(srv *mcpserver.Server, opts Options) {
	cfg, opts, logger, logPath := backfillDefaults(opts)
	ctx := serverContext{srv: srv, opts: opts, cfg: cfg, logger: logger, logPath: logPath}
	registerLibraryTools(&ctx)
	registerProfileTools(&ctx)
	registerRenderTools(&ctx)
	registerVersioningTools(&ctx)
	registerStatusTool(&ctx)
}

// Serve starts the symskills MCP server on stdin/stdout.
func Serve(version string, opts Options) error {
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	srv := mcpserver.New("symskills", version)
	Register(srv, opts)
	return srv.ServeStdio(ctx)
}
