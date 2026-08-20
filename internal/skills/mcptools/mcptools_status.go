package mcptools

import (
	"context"
	"encoding/json"

	"github.com/danieljustus/symaira-brain/internal/skills/harness"
	"github.com/danieljustus/symaira-brain/internal/skills/render"
	"github.com/danieljustus/symaira-corekit/exitcodes"
	"github.com/danieljustus/symaira-corekit/mcpserver"
)

// registerStatusTool registers the read-only status inventory tool:
// skills_targets_status.
func registerStatusTool(ctx *serverContext) {
	opts := ctx.opts
	svc := ctx.srv

	svc.RegisterTool(&mcpserver.Tool{
		Name:        "skills_targets_status",
		Description: "Read-only inventory and readiness status for supported AI-agent harnesses.",
		InputSchema: json.RawMessage(`{"type":"object","properties":{"scope":{"type":"string"}}}`),
		Handler: func(_ context.Context, in json.RawMessage) (any, error) {
			var args struct {
				Scope string `json:"scope"`
			}
			if err := json.Unmarshal(in, &args); err != nil {
				return nil, exitcodes.Wrap(err, exitcodes.ExitData, exitcodes.KindValidation, "parse arguments")
			}
			scope := render.ScopeUser
			if args.Scope == string(render.ScopeProject) {
				scope = render.ScopeProject
			}
			statuses := harness.ListStatus(harness.Options{
				HomeDir:    opts.HomeDir,
				ProjectDir: opts.ProjectDir,
				Scope:      scope,
			})
			return mcpJSON(map[string]any{"targets": statuses})
		},
	})
}
