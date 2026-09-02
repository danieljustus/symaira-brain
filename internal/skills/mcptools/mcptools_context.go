package mcptools

import (
	"context"
	"encoding/json"
	"fmt"
	"path/filepath"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/skills/config"
	"github.com/danieljustus/symaira-brain/internal/skills/events"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
	"github.com/danieljustus/symaira-corekit/exitcodes"
	"github.com/danieljustus/symaira-corekit/mcpserver"
)

// serverContext carries the shared state that every tool-group registration
// function needs: the already-backfilled Options, the resolved config for
// defaults, the event-log path, and the optional logger.
type serverContext struct {
	srv     *mcpserver.Server
	opts    Options
	cfg     *config.Config
	logger  *events.Logger
	logPath string
	// wrap, when non-nil, wraps every tool Handler registered via
	// registerTool (see ToolWrapper). It mirrors opts.Wrap; nil leaves
	// handlers unwrapped.
	wrap ToolWrapper
}

// registerTool registers t on the underlying mcpserver.Server, wrapping its
// Handler through ctx.wrap first when one is set. Every tool-group
// registration function (registerLibraryTools, registerRenderTools, ...)
// calls this instead of ctx.srv.RegisterTool directly so the gateway's
// audit+episode closure reaches every symskills tool call (issue #422).
func (ctx *serverContext) registerTool(t *mcpserver.Tool) {
	if ctx.wrap != nil {
		t.Handler = ctx.wrap(t.Name, t.Handler)
	}
	ctx.srv.RegisterTool(t)
}

// backfillDefaults mirrors the per-field backfill that Register previously
// inlined: empty fields are filled from config.Defaults() so callers that
// pass Options{} (tests) behave like real deployments.
func backfillDefaults(opts Options) (*config.Config, Options, *events.Logger, string) {
	cfg := config.Defaults()
	if opts.LibraryDir == "" {
		opts.LibraryDir = cfg.LibraryDir
	}
	if opts.RenderDir == "" {
		opts.RenderDir = cfg.RenderDir
	}
	if opts.CacheDir == "" {
		opts.CacheDir = cfg.CacheDir
	}
	if opts.BaseDir == "" {
		opts.BaseDir = cfg.BaseDir
	}
	if opts.ProfilesDir == "" {
		opts.ProfilesDir = cfg.ProfilesDir
	}
	if opts.VCSEnabled == nil {
		enabled := cfg.VCSEnabled()
		opts.VCSEnabled = &enabled
	}
	// The operation log is a file, never console output: stdout stays
	// reserved for JSON-RPC frames while serving MCP (AGENTS.md). The
	// logger is only created when a log location is known; the CLI serve
	// command passes EventsPath explicitly, so a bare Options{} (as used
	// by tests) never writes anywhere.
	logPath := opts.EventsPath
	if logPath == "" && opts.HomeDir != "" {
		logPath = filepath.Join(opts.HomeDir, ".local", "share", "symskills", "events.jsonl")
	}
	var logger *events.Logger
	if logPath != "" {
		logger = events.New(logPath, opts.Version)
	}
	return cfg, opts, logger, logPath
}

// callInspect resolves a skill bundle from the MCP input's path or name
// field. It is shared by the library and render tool groups.
func callInspect(_ context.Context, _ *mcpserver.Server, opts Options, in json.RawMessage) (*skill.Bundle, error) {
	var args struct {
		Path string `json:"path"`
		Name string `json:"name"`
	}
	if err := json.Unmarshal(in, &args); err != nil {
		return nil, exitcodes.Wrap(err, exitcodes.ExitData, exitcodes.KindValidation, "parse arguments")
	}
	root := args.Path
	if root == "" && args.Name != "" {
		root = filepath.Join(opts.LibraryDir, args.Name)
	}
	if root == "" {
		return nil, exitcodes.Wrap(fmt.Errorf("path or name is required"), exitcodes.ExitData, exitcodes.KindValidation, "inspect skill")
	}
	return skill.LoadBundle(root)
}

// joinMessages joins non-empty messages with "; ", returning "" for nil.
func joinMessages(messages []string) string {
	if len(messages) == 0 {
		return ""
	}
	return strings.Join(messages, "; ")
}
