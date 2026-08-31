package mcptools

import (
	"context"
	"encoding/json"

	"github.com/danieljustus/symaira-brain/internal/skills/events"
	"github.com/danieljustus/symaira-brain/internal/skills/install"
	"github.com/danieljustus/symaira-brain/internal/skills/metadata"
	"github.com/danieljustus/symaira-brain/internal/skills/render"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
	"github.com/danieljustus/symaira-corekit/mcpserver"
)

// registerLibraryTools registers the library introspection tools:
// skills_list, skills_inspect, and skills_validate.
func registerLibraryTools(ctx *serverContext) {
	opts := ctx.opts
	svc := ctx.srv
	logger := ctx.logger
	logPath := ctx.logPath

	svc.RegisterTool(&mcpserver.Tool{
		Name:        "skills_list",
		Description: "List skills in the symskills library.",
		InputSchema: json.RawMessage(emptyObject),
		Annotations: &mcpserver.ToolAnnotations{Title: "List Skills", ReadOnlyHint: true},
		Handler: func(_ context.Context, _ json.RawMessage) (any, error) {
			bundles, issues := skill.ListLibrary(opts.LibraryDir)
			items := make([]skillListItem, 0, len(bundles))
			categoryCounts := map[string]int{}
			preRead, perr := metadata.ReadEventsLog(logPath)
			if perr != nil {
				preRead = nil // degrade gracefully — same as missing log
			}
			metaOpts := metadata.Options{
				LogPath:    logPath,
				Events:     preRead,
				InstallOpt: install.Options{HomeDir: opts.HomeDir, Scope: render.ScopeUser},
			}
			for _, bundle := range bundles {
				rec := metadata.Collect(bundle.Root, bundle.Frontmatter.Name, metaOpts)
				category := bundle.Frontmatter.Category
				if category != "" {
					categoryCounts[category]++
				}
				items = append(items, skillListItem{
					Name:        bundle.Frontmatter.Name,
					Description: bundle.Frontmatter.Description,
					Category:    category,
					Root:        bundle.Root,
					Record:      rec,
				})
			}
			return mcpJSON(map[string]any{"skills": items, "category_counts": categoryCounts, "issues": issues})
		},
	})
	svc.RegisterTool(&mcpserver.Tool{
		Name:        "skills_inspect",
		Description: "Inspect one skill by path or library name.",
		InputSchema: json.RawMessage(`{"type":"object","properties":{"path":{"type":"string"},"name":{"type":"string"}}}`),
		Annotations: &mcpserver.ToolAnnotations{Title: "Inspect Skill", ReadOnlyHint: true},
		Handler: func(c context.Context, in json.RawMessage) (any, error) {
			bundle, err := callInspect(c, svc, opts, in)
			if err != nil {
				return nil, err
			}
			rec := metadata.Collect(bundle.Root, bundle.Frontmatter.Name, metadata.Options{
				LogPath:    logPath,
				InstallOpt: install.Options{HomeDir: opts.HomeDir, Scope: render.ScopeUser},
			})
			return mcpJSON(struct {
				*skill.Bundle
				metadata.Record
			}{bundle, rec})
		},
	})
	svc.RegisterTool(&mcpserver.Tool{
		Name:        "skills_validate",
		Description: "Validate one skill by path or library name.",
		InputSchema: json.RawMessage(`{"type":"object","properties":{"path":{"type":"string"},"name":{"type":"string"}}}`),
		Annotations: &mcpserver.ToolAnnotations{Title: "Validate Skill", ReadOnlyHint: true},
		Handler: func(c context.Context, in json.RawMessage) (any, error) {
			result, err := callInspect(c, svc, opts, in)
			if err != nil {
				return nil, err
			}
			issues := skill.Validate(result)
			hasErrors := false
			messages := make([]string, 0, len(issues))
			for _, issue := range issues {
				if issue.Severity == "error" {
					hasErrors = true
					messages = append(messages, issue.Message)
				}
			}
			if hasErrors && logger != nil {
				logger.Record(events.Event{
					Event:   events.EventValidateFailure,
					Skill:   result.Frontmatter.Name,
					Path:    result.Root,
					Outcome: events.OutcomeError,
					Error:   joinMessages(messages),
					Actor:   events.ActorMCP,
				})
			}
			return mcpJSON(map[string]any{"valid": !hasErrors, "issues": issues})
		},
	})
}
