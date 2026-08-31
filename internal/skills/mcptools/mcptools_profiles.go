package mcptools

import (
	"context"
	"encoding/json"

	"github.com/danieljustus/symaira-brain/internal/skills/profile"
	"github.com/danieljustus/symaira-corekit/exitcodes"
	"github.com/danieljustus/symaira-corekit/mcpserver"
)

// registerProfileTools registers the context-profile tools:
// skills_profile_list and skills_profile_resolve.
func registerProfileTools(ctx *serverContext) {
	opts := ctx.opts
	svc := ctx.srv

	svc.RegisterTool(&mcpserver.Tool{
		Name:        "skills_profile_list",
		Description: "List available context profiles (global and project).",
		InputSchema: json.RawMessage(emptyObject),
		Annotations: &mcpserver.ToolAnnotations{Title: "List Skill Profiles", ReadOnlyHint: true},
		Handler: func(_ context.Context, _ json.RawMessage) (any, error) {
			refs, err := profile.List(opts.ProfilesDir, opts.ProjectDir)
			if err != nil {
				return nil, exitcodes.Wrap(err, exitcodes.ExitData, exitcodes.KindValidation, "list profiles")
			}
			return mcpJSON(map[string]any{"profiles": refs})
		},
	})
	svc.RegisterTool(&mcpserver.Tool{
		Name:        "skills_profile_resolve",
		Description: "Resolve a context profile and return the merged skill set.",
		InputSchema: json.RawMessage(`{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}`),
		Annotations: &mcpserver.ToolAnnotations{Title: "Resolve Skill Profile", ReadOnlyHint: true},
		Handler: func(_ context.Context, in json.RawMessage) (any, error) {
			var args struct {
				Name string `json:"name"`
			}
			if err := json.Unmarshal(in, &args); err != nil {
				return nil, exitcodes.Wrap(err, exitcodes.ExitData, exitcodes.KindValidation, "parse arguments")
			}
			resolved, issues, err := profile.Resolve(opts.LibraryDir, opts.ProfilesDir, opts.ProjectDir, args.Name)
			if err != nil {
				return nil, exitcodes.Wrap(err, exitcodes.ExitData, exitcodes.KindValidation, "resolve profile")
			}
			return mcpJSON(map[string]any{"skills": resolved, "issues": issues})
		},
	})
}
