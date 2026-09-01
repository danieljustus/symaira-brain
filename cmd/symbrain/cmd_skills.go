package main

import (
	"flag"
	"fmt"
	"io"
	"os"
	"sort"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/output"
	"github.com/danieljustus/symaira-brain/internal/skills/config"
	"github.com/danieljustus/symaira-brain/internal/skills/events"
	"github.com/danieljustus/symaira-brain/internal/skills/harness"
	"github.com/danieljustus/symaira-brain/internal/skills/install"
	"github.com/danieljustus/symaira-brain/internal/skills/metadata"
	"github.com/danieljustus/symaira-brain/internal/skills/render"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// cmdSkills dispatches the embedded skills core subcommands. Like memory,
// skills runs inside this binary — every subcommand is routed through the
// normal dispatcher here and never through cmdPassthrough.
func cmdSkillsWithFormat(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	if len(args) == 0 {
		printSkillsUsage(stderr)
		return exitcodes.ExitNoInput
	}
	switch args[0] {
	case "-h", "--help":
		printSkillsUsage(stdout)
		return exitcodes.ExitOK
	case "list":
		return cmdSkillsList(args[1:], stdout, stderr, format)
	case "status":
		return cmdSkillsStatus(args[1:], stdout, stderr, format)
	case "targets":
		return cmdSkillsTargets(args[1:], stdout, stderr, format)
	case "log":
		return cmdSkillsLog(args[1:], stdout, stderr, format)
	case "sync":
		return cmdSkillsSync(args[1:], stdout, stderr, format)
	case "doctor":
		return cmdSkillsDoctor(args[1:], stdout, stderr, format)
	default:
		fmt.Fprintf(stderr, "symbrain skills: unknown subcommand %q\n\n", args[0])
		printSkillsUsage(stderr)
		return exitcodes.ExitNoInput
	}
}

// skillsEnv is the resolved environment every skills subcommand works
// against: the loaded config plus the home/project directories that decide
// where a harness keeps its skill root.
type skillsEnv struct {
	cfg        *config.Config
	homeDir    string
	projectDir string
	logPath    string
}

// loadSkillsEnv resolves config and directories. A missing or unreadable
// config file is not fatal — the packaged defaults describe the same XDG
// layout the library actually lives in.
func loadSkillsEnv() skillsEnv {
	cfg, err := config.Load()
	if err != nil || cfg == nil {
		cfg = config.Defaults()
	}
	env := skillsEnv{cfg: cfg, logPath: events.DefaultPath()}
	if home, err := os.UserHomeDir(); err == nil {
		env.homeDir = home
	}
	if cwd, err := os.Getwd(); err == nil {
		env.projectDir = cwd
	}
	return env
}

func (e skillsEnv) statusOptions(targets []render.Target, scope render.Scope) install.StatusOptions {
	return install.StatusOptions{
		HomeDir:    e.homeDir,
		ProjectDir: e.projectDir,
		Scope:      scope,
		LibraryDir: e.cfg.LibraryDir,
		BaseDir:    e.cfg.BaseDir,
		CacheDir:   e.cfg.CacheDir,
		Targets:    targets,
	}
}

// skillsFlagSet builds the flag set shared by the skills subcommands.
// Subcommands ignore the flags they do not use; a single definition keeps
// `--target` and `--scope` spelled the same everywhere.
func skillsFlagSet(name string, stderr io.Writer) (*flag.FlagSet, *string, *string) {
	fs := flag.NewFlagSet("skills "+name, flag.ContinueOnError)
	fs.SetOutput(stderr)
	target := fs.String("target", "", "limit to one harness target")
	scope := fs.String("scope", string(render.ScopeUser), "install scope: user or project")
	return fs, target, scope
}

// resolveTargets turns a --target value into the target filter Status and
// Sync take. An empty value means every registered target.
func resolveTargets(value string) ([]render.Target, error) {
	value = strings.TrimSpace(value)
	if value == "" {
		return nil, nil
	}
	for _, spec := range render.Targets {
		if string(spec.Name) == value {
			return []render.Target{spec.Name}, nil
		}
	}
	names := make([]string, 0, len(render.Targets))
	for _, spec := range render.Targets {
		names = append(names, string(spec.Name))
	}
	return nil, fmt.Errorf("unknown target %q (known: %s)", value, strings.Join(names, ", "))
}

func resolveScope(value string) (render.Scope, error) {
	switch render.Scope(strings.TrimSpace(value)) {
	case "", render.ScopeUser:
		return render.ScopeUser, nil
	case render.ScopeProject:
		return render.ScopeProject, nil
	default:
		return "", fmt.Errorf("unknown scope %q (known: user, project)", value)
	}
}

// ---- list ----

// skillLibraryEntry is one row of `symbrain skills list`: the frontmatter
// summary plus the per-skill metadata record (render/install/last-used).
type skillLibraryEntry struct {
	Name        string `json:"name"`
	Description string `json:"description,omitempty"`
	Category    string `json:"category,omitempty"`
	Path        string `json:"path"`
	metadata.Record
}

type skillLibraryReport struct {
	Skills         []skillLibraryEntry `json:"skills"`
	CategoryCounts map[string]int      `json:"category_counts"`
	Issues         []skill.Issue       `json:"issues"`
}

func cmdSkillsList(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	fs, _, _ := skillsFlagSet("list", stderr)
	if err := fs.Parse(normalizeFlags(args)); err != nil {
		return exitcodes.ExitNoInput
	}
	env := loadSkillsEnv()

	bundles, issues := skill.ListLibrary(env.cfg.LibraryDir)
	if issues == nil {
		issues = make([]skill.Issue, 0)
	}
	preRead, err := metadata.ReadEventsLog(env.logPath)
	if err != nil {
		preRead = nil // degrade gracefully — same as a missing log
	}
	metaOpts := metadata.Options{
		LogPath:    env.logPath,
		Events:     preRead,
		InstallOpt: install.Options{HomeDir: env.homeDir, Scope: render.ScopeUser},
	}

	report := skillLibraryReport{
		Skills:         make([]skillLibraryEntry, 0, len(bundles)),
		CategoryCounts: map[string]int{},
		Issues:         issues,
	}
	for _, bundle := range bundles {
		if bundle == nil {
			continue
		}
		if bundle.Frontmatter.Category != "" {
			report.CategoryCounts[bundle.Frontmatter.Category]++
		}
		report.Skills = append(report.Skills, skillLibraryEntry{
			Name:        bundle.Frontmatter.Name,
			Description: bundle.Frontmatter.Description,
			Category:    bundle.Frontmatter.Category,
			Path:        bundle.Root,
			Record:      metadata.Collect(bundle.Root, bundle.Frontmatter.Name, metaOpts),
		})
	}

	rows := output.Rows{
		JSON: report,
		Table: func(w io.Writer) error {
			if len(report.Skills) == 0 {
				_, err := fmt.Fprintln(w, "No skills in the library.")
				return err
			}
			if _, err := fmt.Fprintln(w, "NAME\tCATEGORY\tINSTALLS\tDESCRIPTION"); err != nil {
				return err
			}
			for _, entry := range report.Skills {
				targets := make([]string, 0, len(entry.Installs))
				for _, in := range entry.Installs {
					targets = append(targets, in.Target)
				}
				sort.Strings(targets)
				installed := strings.Join(targets, ",")
				if installed == "" {
					installed = "-"
				}
				if _, err := fmt.Fprintf(w, "%s\t%s\t%s\t%s\n", entry.Name,
					orDash(entry.Category), installed, tableMemoryContent(entry.Description)); err != nil {
					return err
				}
			}
			return nil
		},
	}
	return renderSkills(stdout, stderr, "list", format, rows)
}

// ---- status ----

type skillStatusSummary struct {
	InSync         int `json:"in_sync"`
	Stale          int `json:"stale"`
	HarnessChanged int `json:"harness_changed"`
	Conflict       int `json:"conflict"`
	Orphaned       int `json:"orphaned"`
	Unmanaged      int `json:"unmanaged"`
}

type skillStatusReport struct {
	Installs []install.InstallStatus `json:"installs"`
	Summary  skillStatusSummary      `json:"summary"`
}

func cmdSkillsStatus(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	fs, target, scopeFlag := skillsFlagSet("status", stderr)
	if err := fs.Parse(normalizeFlags(args)); err != nil {
		return exitcodes.ExitNoInput
	}
	targets, err := resolveTargets(*target)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain skills status: %v\n", err)
		return exitcodes.ExitNoInput
	}
	scope, err := resolveScope(*scopeFlag)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain skills status: %v\n", err)
		return exitcodes.ExitNoInput
	}

	env := loadSkillsEnv()
	statuses, err := install.Status(env.statusOptions(targets, scope))
	if err != nil {
		fmt.Fprintf(stderr, "symbrain skills status: scan installs: %v\n", err)
		return exitcodes.ExitGeneric
	}
	if statuses == nil {
		statuses = make([]install.InstallStatus, 0)
	}

	report := skillStatusReport{Installs: statuses}
	for _, st := range statuses {
		switch st.Status {
		case install.StatusInSync:
			report.Summary.InSync++
		case install.StatusStale:
			report.Summary.Stale++
		case install.StatusHarnessChanged:
			report.Summary.HarnessChanged++
		case install.StatusConflict:
			report.Summary.Conflict++
		case install.StatusOrphaned:
			report.Summary.Orphaned++
		case install.StatusUnmanaged:
			report.Summary.Unmanaged++
		}
	}

	rows := output.Rows{
		JSON: report,
		Table: func(w io.Writer) error {
			if len(statuses) == 0 {
				_, err := fmt.Fprintln(w, "No installed skills found.")
				return err
			}
			if _, err := fmt.Fprintln(w, "TARGET\tSKILL\tSTATUS\tMODE\tPATH"); err != nil {
				return err
			}
			for _, st := range statuses {
				if _, err := fmt.Fprintf(w, "%s\t%s\t%s\t%s\t%s\n", st.Target, st.Name,
					st.Status, orDash(string(st.Mode)), st.Path); err != nil {
					return err
				}
			}
			return nil
		},
	}
	return renderSkills(stdout, stderr, "status", format, rows)
}

// ---- targets ----

type skillTargetsReport struct {
	Targets []harness.Status `json:"targets"`
}

func cmdSkillsTargets(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	fs, _, scopeFlag := skillsFlagSet("targets", stderr)
	if err := fs.Parse(normalizeFlags(args)); err != nil {
		return exitcodes.ExitNoInput
	}
	scope, err := resolveScope(*scopeFlag)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain skills targets: %v\n", err)
		return exitcodes.ExitNoInput
	}

	env := loadSkillsEnv()
	statuses := harness.ListStatus(harness.Options{
		HomeDir:    env.homeDir,
		ProjectDir: env.projectDir,
		Scope:      scope,
	})
	if statuses == nil {
		statuses = make([]harness.Status, 0)
	}

	rows := output.Rows{
		JSON: skillTargetsReport{Targets: statuses},
		Table: func(w io.Writer) error {
			if _, err := fmt.Fprintln(w, "TARGET\tINSTALLED\tMANAGED\tUNMANAGED\tSKILL ROOT"); err != nil {
				return err
			}
			for _, st := range statuses {
				if _, err := fmt.Fprintf(w, "%s\t%t\t%d\t%d\t%s\n", st.Target, st.Installed,
					st.ManagedSkillsCount, st.UnmanagedSkillsCount, st.EffectiveSkillRoot); err != nil {
					return err
				}
			}
			return nil
		},
	}
	return renderSkills(stdout, stderr, "targets", format, rows)
}

// ---- log ----

func cmdSkillsLog(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	fs, target, _ := skillsFlagSet("log", stderr)
	skillName := fs.String("skill", "", "limit to one skill name")
	limit := fs.Int("limit", 0, "maximum records to return, newest first")
	fs.IntVar(limit, "l", 0, "maximum records to return, newest first")
	if err := fs.Parse(normalizeFlags(args)); err != nil {
		return exitcodes.ExitNoInput
	}

	env := loadSkillsEnv()
	records, err := events.New(env.logPath, version).Read(events.Filter{
		Skill:  strings.TrimSpace(*skillName),
		Target: strings.TrimSpace(*target),
	})
	if err != nil {
		fmt.Fprintf(stderr, "symbrain skills log: read operation log: %v\n", err)
		return exitcodes.ExitGeneric
	}
	if records == nil {
		records = make([]events.Event, 0)
	}
	// Newest first, then bounded — the log is append-only, so the tail is
	// the interesting end.
	sort.SliceStable(records, func(i, j int) bool { return records[i].TS > records[j].TS })
	if *limit > 0 && len(records) > *limit {
		records = records[:*limit]
	}

	rows := output.Rows{
		JSON: records,
		Table: func(w io.Writer) error {
			if len(records) == 0 {
				_, err := fmt.Fprintln(w, "No recorded skill operations.")
				return err
			}
			if _, err := fmt.Fprintln(w, "WHEN\tEVENT\tSKILL\tTARGET\tOUTCOME"); err != nil {
				return err
			}
			for _, ev := range records {
				if _, err := fmt.Fprintf(w, "%s\t%s\t%s\t%s\t%s\n", ev.TS, ev.Event,
					orDash(ev.Skill), orDash(ev.Target), ev.Outcome); err != nil {
					return err
				}
			}
			return nil
		},
	}
	return renderSkills(stdout, stderr, "log", format, rows)
}

// ---- sync ----

type skillSyncReport struct {
	Results []install.SyncResult `json:"results"`
	DryRun  bool                 `json:"dry_run"`
}

func cmdSkillsSync(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	fs, target, scopeFlag := skillsFlagSet("sync", stderr)
	dryRun := fs.Bool("dry-run", false, "report the plan without writing")
	if err := fs.Parse(normalizeFlags(args)); err != nil {
		return exitcodes.ExitNoInput
	}
	targets, err := resolveTargets(*target)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain skills sync: %v\n", err)
		return exitcodes.ExitNoInput
	}
	scope, err := resolveScope(*scopeFlag)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain skills sync: %v\n", err)
		return exitcodes.ExitNoInput
	}

	env := loadSkillsEnv()
	statuses, err := install.Status(env.statusOptions(targets, scope))
	if err != nil {
		fmt.Fprintf(stderr, "symbrain skills sync: scan installs: %v\n", err)
		return exitcodes.ExitGeneric
	}

	results := install.Sync(statuses, install.SyncOptions{
		LibraryDir:     env.cfg.LibraryDir,
		RenderDir:      env.cfg.RenderDir,
		BaseDir:        env.cfg.BaseDir,
		Scope:          scope,
		ConflictPolicy: install.ConflictAbort,
		DryRun:         *dryRun,
		HomeDir:        env.homeDir,
	})
	if results == nil {
		results = make([]install.SyncResult, 0)
	}

	rows := output.Rows{
		JSON: skillSyncReport{Results: results, DryRun: *dryRun},
		Table: func(w io.Writer) error {
			if len(results) == 0 {
				_, err := fmt.Fprintln(w, "Every installed skill is in sync.")
				return err
			}
			if _, err := fmt.Fprintln(w, "TARGET\tSKILL\tACTION\tDETAIL"); err != nil {
				return err
			}
			for _, res := range results {
				if _, err := fmt.Fprintf(w, "%s\t%s\t%s\t%s\n", res.Target, res.Name,
					res.Action, orDash(res.Error)); err != nil {
					return err
				}
			}
			return nil
		},
	}
	return renderSkills(stdout, stderr, "sync", format, rows)
}

// ---- doctor ----

type skillsDoctorTarget struct {
	Target  string `json:"target"`
	User    string `json:"user"`
	Project string `json:"project"`
}

type skillsDoctorReport struct {
	Config      *config.Config       `json:"config"`
	ConfigPath  string               `json:"config_path"`
	LogPath     string               `json:"log_path"`
	ProfilesDir string               `json:"profiles_dir"`
	ProjectDir  string               `json:"project_dir"`
	Targets     []skillsDoctorTarget `json:"targets"`
}

func cmdSkillsDoctor(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	fs, _, _ := skillsFlagSet("doctor", stderr)
	if err := fs.Parse(normalizeFlags(args)); err != nil {
		return exitcodes.ExitNoInput
	}

	env := loadSkillsEnv()
	report := skillsDoctorReport{
		Config:      env.cfg,
		ConfigPath:  config.ConfigPath(),
		LogPath:     env.logPath,
		ProfilesDir: env.cfg.ProfilesDir,
		ProjectDir:  env.projectDir,
		Targets:     make([]skillsDoctorTarget, 0, len(render.Targets)),
	}
	for _, spec := range render.Targets {
		report.Targets = append(report.Targets, skillsDoctorTarget{
			Target:  string(spec.Name),
			User:    spec.SkillRoot(env.homeDir, env.projectDir, render.ScopeUser),
			Project: spec.SkillRoot(env.homeDir, env.projectDir, render.ScopeProject),
		})
	}

	rows := output.Rows{
		JSON: report,
		Table: func(w io.Writer) error {
			pairs := [][2]string{
				{"config", report.ConfigPath},
				{"library", env.cfg.LibraryDir},
				{"rendered", env.cfg.RenderDir},
				{"cache", env.cfg.CacheDir},
				{"base", env.cfg.BaseDir},
				{"profiles", env.cfg.ProfilesDir},
				{"log", report.LogPath},
				{"project", report.ProjectDir},
				{"versioning", fmt.Sprintf("%t", env.cfg.VCSEnabled())},
			}
			for _, pair := range pairs {
				if _, err := fmt.Fprintf(w, "%-11s %s\n", pair[0], orDash(pair[1])); err != nil {
					return err
				}
			}
			return nil
		},
	}
	return renderSkills(stdout, stderr, "doctor", format, rows)
}

// ---- helpers ----

func renderSkills(stdout, stderr io.Writer, subcommand string, format output.Format, rows output.Rows) exitcodes.ExitCode {
	if err := output.Render(stdout, format, rows); err != nil {
		fmt.Fprintf(stderr, "symbrain skills %s: format output: %v\n", subcommand, err)
		return exitcodes.ExitGeneric
	}
	return exitcodes.ExitOK
}

func orDash(value string) string {
	if strings.TrimSpace(value) == "" {
		return "-"
	}
	return value
}

func printSkillsUsage(w io.Writer) {
	fmt.Fprint(w, `symbrain skills — embedded skill library operations

Usage:
  symbrain skills <subcommand> [flags]

Subcommands:
  list        List the skills in the library with their install state
  status      Classify installed skills against the library (drift report)
  targets     Show the harness targets skills can be installed into
  log         Read the local skill operation log
  sync        Repair drifted installs (use --dry-run to see the plan first)
  doctor      Report the configured skill paths and target roots

Use --output table|json (or --json) for the result format. status, sync and
targets accept --scope user|project; status, sync and log accept --target.
Run 'symbrain skills <subcommand> --help' for details.
`)
}
