package mcptools

import (
	"context"
	"encoding/json"
	"fmt"
	"path/filepath"

	"github.com/danieljustus/symaira-brain/internal/skills/config"
	"github.com/danieljustus/symaira-brain/internal/skills/events"
	"github.com/danieljustus/symaira-brain/internal/skills/install"
	"github.com/danieljustus/symaira-brain/internal/skills/profile"
	"github.com/danieljustus/symaira-brain/internal/skills/render"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
	"github.com/danieljustus/symaira-corekit/exitcodes"
	"github.com/danieljustus/symaira-corekit/mcpserver"
)

// registerRenderTools registers the render/install tools:
// skills_render_plan and skills_install.
func registerRenderTools(ctx *serverContext) {
	opts := ctx.opts
	svc := ctx.srv
	cfg := ctx.cfg
	logger := ctx.logger

	svc.RegisterTool(&mcpserver.Tool{
		Name:        "skills_render_plan",
		Description: "Render a skill or profile to the managed artifact directory and return planned target paths. Pass dry_run=true to preview without writing.",
		InputSchema: json.RawMessage(`{"type":"object","properties":{"path":{"type":"string"},"name":{"type":"string"},"target":{"type":"string"},"profile":{"type":"string"},"dry_run":{"type":"boolean"}}}`),
		Annotations: &mcpserver.ToolAnnotations{Title: "Render Skill Plan", ReadOnlyHint: false, DestructiveHint: true},
		Handler: func(c context.Context, in json.RawMessage) (any, error) {
			var args struct {
				Target  string `json:"target"`
				Profile string `json:"profile"`
				DryRun  *bool  `json:"dry_run"`
			}
			if err := json.Unmarshal(in, &args); err != nil {
				return nil, exitcodes.Wrap(err, exitcodes.ExitData, exitcodes.KindValidation, "parse arguments")
			}
			targets := render.DefaultTargets()
			if args.Target != "" {
				target, err := render.ParseTarget(args.Target)
				if err != nil {
					return nil, err
				}
				targets = []render.Target{target}
			}
			dryRun := false
			if args.DryRun != nil {
				dryRun = *args.DryRun
			}

			if args.Profile != "" {
				if dryRun {
					return renderProfileDryRun(opts, targets, args.Profile)
				}
				return renderProfile(opts, cfg, targets, args.Profile, dryRun, logger)
			}
			bundle, err := callInspect(c, svc, opts, in)
			if err != nil {
				return nil, err
			}

			if dryRun {
				// Plan only: no writes to render directory.
				planned := make([]render.Rendered, 0, len(targets))
				var errs []error
				for _, target := range targets {
					item, err := render.RenderTarget(bundle, target)
					if err != nil {
						errs = append(errs, fmt.Errorf("target %s: %w", target, err))
						continue
					}
					item.Path = filepath.Join(opts.RenderDir, string(target), item.Name)
					planned = append(planned, item)
				}
				if len(planned) == 0 && len(errs) > 0 {
					return nil, errs[0]
				}
				return mcpJSON(planned)
			}

			rendered, errs := render.RenderAll(bundle, opts.RenderDir, targets)
			if len(rendered) == 0 && len(errs) > 0 {
				logger.Record(events.Event{Event: events.EventRender, Skill: bundle.Frontmatter.Name, Target: args.Target, Outcome: events.OutcomeError, Error: errs[0].Error(), Actor: events.ActorMCP})
				return nil, errs[0]
			}
			targetLabel := args.Target
			if targetLabel == "" {
				targetLabel = "all"
			}
			logger.Record(events.Event{Event: events.EventRender, Skill: bundle.Frontmatter.Name, SkillVersion: bundle.Frontmatter.Version, Target: targetLabel, Path: opts.RenderDir, Outcome: events.OutcomeOK, Actor: events.ActorMCP})
			return mcpJSON(rendered)
		},
	})
	svc.RegisterTool(&mcpserver.Tool{
		Name:        "skills_install",
		Description: "Render and install a skill or profile. Dry-run defaults to true; pass dry_run=false for writes.",
		InputSchema: json.RawMessage(`{"type":"object","properties":{"path":{"type":"string"},"name":{"type":"string"},"target":{"type":"string"},"scope":{"type":"string"},"dry_run":{"type":"boolean"},"profile":{"type":"string"}}}`),
		Annotations: &mcpserver.ToolAnnotations{Title: "Install Skill", ReadOnlyHint: false, DestructiveHint: true},
		Handler: func(c context.Context, in json.RawMessage) (any, error) {
			var args struct {
				Target  string `json:"target"`
				Scope   string `json:"scope"`
				DryRun  *bool  `json:"dry_run"`
				Profile string `json:"profile"`
			}
			if err := json.Unmarshal(in, &args); err != nil {
				return nil, exitcodes.Wrap(err, exitcodes.ExitData, exitcodes.KindValidation, "parse arguments")
			}
			target := render.TargetOpenCode
			if args.Target != "" {
				parsed, err := render.ParseTarget(args.Target)
				if err != nil {
					return nil, err
				}
				target = parsed
			}
			scope := render.ScopeUser
			if args.Scope == string(render.ScopeProject) {
				scope = render.ScopeProject
			}
			dryRun := true
			if args.DryRun != nil {
				dryRun = *args.DryRun
			}
			installOpts := install.Options{HomeDir: opts.HomeDir, ProjectDir: opts.ProjectDir, Scope: scope, DryRun: dryRun}

			if args.Profile != "" {
				if dryRun {
					return installProfileDryRun(opts, target, args.Profile, installOpts)
				}
				return installProfile(opts, cfg, target, args.Profile, installOpts, logger)
			}
			bundle, err := callInspect(c, svc, opts, in)
			if err != nil {
				return nil, err
			}

			if dryRun {
				// Plan only: compute render target without writing to render dir.
				item, err := render.RenderTarget(bundle, target)
				if err != nil {
					return nil, err
				}
				item.Path = filepath.Join(opts.RenderDir, string(target), item.Name)
				dest, err := install.InstallPath(target, item.Name, installOpts)
				if err != nil {
					return nil, err
				}
				return mcpJSON(install.Result{
					Action: "planned",
					Target: target,
					Name:   item.Name,
					Path:   dest,
					Mode:   installOpts.Mode,
				})
			}

			rendered, errs := render.RenderAll(bundle, opts.RenderDir, []render.Target{target})
			if len(rendered) == 0 {
				if len(errs) > 0 {
					logger.Record(events.Event{Event: events.EventInstall, Skill: bundle.Frontmatter.Name, Target: string(target), Outcome: events.OutcomeError, Error: errs[0].Error(), Actor: events.ActorMCP})
					return nil, errs[0]
				}
				return nil, fmt.Errorf("target %s produced no render output", target)
			}
			result, err := install.Install(install.RenderedSkill{
				Target: target,
				Name:   rendered[0].Name,
				Path:   rendered[0].Path,
			}, installOpts)
			if err != nil {
				logger.Record(events.Event{Event: events.EventInstall, Skill: rendered[0].Name, Target: string(target), Outcome: events.OutcomeError, Error: err.Error(), Actor: events.ActorMCP})
				return nil, err
			}
			logger.Record(events.Event{Event: events.EventInstall, Skill: result.Name, Target: string(result.Target), Scope: string(installOpts.Scope), Mode: string(result.Mode), Path: result.Path, Outcome: events.OutcomeOK, Actor: events.ActorMCP})
			return mcpJSON(result)
		},
	})
}

// renderProfile resolves and renders a context profile across the given
// targets, writing to the render directory.
func renderProfile(opts Options, _ *config.Config, targets []render.Target, profileName string, dryRun bool, logger *events.Logger) (any, error) {
	if dryRun {
		return renderProfileDryRun(opts, targets, profileName)
	}
	_ = logger
	results, issues, err := profile.RenderProfile(opts.LibraryDir, opts.ProfilesDir, opts.ProjectDir, opts.RenderDir, targets, profileName)
	if err != nil {
		return nil, exitcodes.Wrap(err, exitcodes.ExitData, exitcodes.KindValidation, "resolve profile")
	}
	if len(issues) > 0 {
		return mcpJSON(map[string]any{"skills": []render.Rendered{}, "issues": issues})
	}
	return mcpJSON(results)
}

// renderProfileDryRun plans profile rendering without writing to disk.
func renderProfileDryRun(opts Options, targets []render.Target, profileName string) (any, error) {
	resolved, issues, err := profile.Resolve(opts.LibraryDir, opts.ProfilesDir, opts.ProjectDir, profileName)
	if err != nil {
		return nil, exitcodes.Wrap(err, exitcodes.ExitData, exitcodes.KindValidation, "resolve profile")
	}
	if len(issues) > 0 {
		return mcpJSON(map[string]any{"skills": []render.Rendered{}, "issues": issues})
	}
	var planned []render.Rendered
	for _, rs := range resolved {
		bundle, err := skill.LoadBundle(filepath.Join(opts.LibraryDir, rs.Skill))
		if err != nil {
			return nil, fmt.Errorf("profile link %q: %w", rs.Name, err)
		}
		for _, target := range targets {
			item, err := render.RenderTarget(bundle, target, render.RenderMeta{Source: rs.Source, Profile: rs.Profile})
			if err != nil {
				continue // skip targets this skill doesn't support
			}
			item.Path = filepath.Join(opts.RenderDir, string(target), item.Name)
			planned = append(planned, item)
		}
	}
	return mcpJSON(planned)
}

// installProfile installs all skill bundles resolved from a context profile.
func installProfile(opts Options, _ *config.Config, target render.Target, profileName string, installOpts install.Options, logger *events.Logger) (any, error) {
	results, issues, err := profile.InstallProfile(opts.LibraryDir, opts.ProfilesDir, opts.ProjectDir, opts.RenderDir, target, profileName, installOpts)
	if err != nil {
		logger.Record(events.Event{Event: events.EventProfileInstall, Target: string(target), Outcome: events.OutcomeError, Error: err.Error(), Actor: events.ActorMCP})
		return nil, exitcodes.Wrap(err, exitcodes.ExitData, exitcodes.KindValidation, "resolve profile")
	}
	if len(issues) > 0 {
		messages := make([]string, 0, len(issues))
		for _, issue := range issues {
			messages = append(messages, issue.Message)
		}
		logger.Record(events.Event{Event: events.EventProfileInstall, Target: string(target), Outcome: events.OutcomeError, Error: joinMessages(messages), Actor: events.ActorMCP})
		return mcpJSON(map[string]any{"results": []install.Result{}, "issues": issues})
	}
	for _, result := range results {
		logger.Record(events.Event{Event: events.EventProfileInstall, Skill: result.Name, Target: string(result.Target), Scope: string(installOpts.Scope), Mode: string(result.Mode), Path: result.Path, Outcome: events.OutcomeOK, Actor: events.ActorMCP})
	}
	return mcpJSON(results)
}

// installProfileDryRun plans profile install without writing to render or install directories.
func installProfileDryRun(opts Options, target render.Target, profileName string, installOpts install.Options) (any, error) {
	resolved, issues, err := profile.Resolve(opts.LibraryDir, opts.ProfilesDir, opts.ProjectDir, profileName)
	if err != nil {
		return nil, exitcodes.Wrap(err, exitcodes.ExitData, exitcodes.KindValidation, "resolve profile")
	}
	if len(issues) > 0 {
		return mcpJSON(map[string]any{"results": []install.Result{}, "issues": issues})
	}
	var results []install.Result
	for _, rs := range resolved {
		bundle, err := skill.LoadBundle(filepath.Join(opts.LibraryDir, rs.Skill))
		if err != nil {
			return nil, fmt.Errorf("profile link %q: %w", rs.Name, err)
		}
		item, err := render.RenderTarget(bundle, target, render.RenderMeta{Source: rs.Source, Profile: rs.Profile})
		if err != nil {
			continue
		}
		item.Path = filepath.Join(opts.RenderDir, string(target), item.Name)
		dest, err := install.InstallPath(target, item.Name, installOpts)
		if err != nil {
			return nil, err
		}
		results = append(results, install.Result{
			Action: "planned",
			Target: target,
			Name:   item.Name,
			Path:   dest,
			Mode:   installOpts.Mode,
		})
	}
	return mcpJSON(results)
}
