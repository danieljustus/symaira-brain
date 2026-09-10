package install

import (
	"path/filepath"

	"github.com/danieljustus/symaira-brain/internal/skills/events"
	"github.com/danieljustus/symaira-brain/internal/skills/render"
)

func recordInstallEvent(item RenderedSkill, opts Options, result Result, err error) {
	if opts.EventsPath == "" || opts.DryRun {
		return
	}
	target := item.Target
	name := item.Name
	path := result.Path
	mode := opts.Mode
	if mode == "" {
		mode = ModeSymlink
	}
	if path == "" {
		path, _ = InstallPath(target, name, opts)
	}
	outcome := events.OutcomeOK
	detail := ""
	if err != nil {
		outcome = events.OutcomeError
		detail = err.Error()
	}
	actor := opts.EventActor
	if actor == "" {
		actor = events.ActorCLI
	}
	events.New(opts.EventsPath, opts.EventToolVersion).Record(events.Event{
		Event:   events.EventInstall,
		Skill:   name,
		Target:  string(target),
		Scope:   string(opts.Scope),
		Mode:    string(mode),
		Path:    filepath.Clean(path),
		Outcome: outcome,
		Error:   detail,
		Actor:   actor,
	})
}

func recordUninstallEvent(target render.Target, name string, opts Options, path string, err error) {
	if opts.EventsPath == "" || opts.DryRun {
		return
	}
	outcome := events.OutcomeOK
	detail := ""
	if err != nil {
		outcome = events.OutcomeError
		detail = err.Error()
	}
	actor := opts.EventActor
	if actor == "" {
		actor = events.ActorCLI
	}
	if path == "" {
		path, _ = InstallPath(target, name, opts)
	}
	events.New(opts.EventsPath, opts.EventToolVersion).Record(events.Event{
		Event:   events.EventUninstall,
		Skill:   name,
		Target:  string(target),
		Scope:   string(opts.Scope),
		Mode:    string(opts.Mode),
		Path:    filepath.Clean(path),
		Outcome: outcome,
		Error:   detail,
		Actor:   actor,
	})
}
