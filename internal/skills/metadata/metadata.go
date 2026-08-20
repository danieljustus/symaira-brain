// Package metadata assembles the per-skill lifecycle record exposed by
// `symskills list --json`, `symskills inspect` and the skills_list /
// skills_inspect MCP tools.
//
// Every field is derived, never guessed, and degrades to empty when its
// source is absent:
//
//   - created_at / modified_at come from the filesystem for now (directory
//     and newest-file mtimes); they will switch to per-skill git history
//     once that lands, with the filesystem remaining the fallback.
//   - last_rendered_at and installs[] come from the lifecycle event log
//     (internal/events); when the log is missing, the per-target install
//     marker (.symskills.json) is used as a fallback.
//   - last_used is a best-effort signal with an explicitly named source:
//     the access time of an installed copy where the filesystem records a
//     useful one (atime newer than the file's own mtime), or an opt-in
//     UsageProbe adapter reading a harness's own records. When there is no
//     evidence, last_used stays null — a wrong "last used" is worse than
//     none.
package metadata

import (
	"encoding/json"
	"os"
	"path/filepath"
	"sort"
	"time"

	"github.com/danieljustus/symaira-brain/internal/skills/events"
	"github.com/danieljustus/symaira-brain/internal/skills/install"
	"github.com/danieljustus/symaira-brain/internal/skills/render"
)

// markerFile matches internal/install's marker name; it is read-only here.
const markerFile = ".symskills.json"

// Install is one known installation of a skill for a harness target.
type Install struct {
	Target      string `json:"target"`
	Path        string `json:"path"`
	InstalledAt string `json:"installed_at"`
}

// Record is the queryable metadata record for one skill. Timestamps are
// RFC3339 UTC strings. LastUsed is null when no usage evidence exists —
// never a substituted timestamp.
type Record struct {
	CreatedAt      string     `json:"created_at,omitempty"`
	ModifiedAt     string     `json:"modified_at,omitempty"`
	LastRenderedAt string     `json:"last_rendered_at,omitempty"`
	Installs       []Install  `json:"installs"` // always an array, never null
	LastUsed       *time.Time `json:"last_used"`
	LastUsedSource string     `json:"last_used_source,omitempty"`
}

// Options controls where Collect looks for evidence.
type Options struct {
	// LogPath is the lifecycle event log location. Empty disables
	// log-derived fields (last_rendered_at, installs from events).
	LogPath string
	// Events, when non-nil, is a pre-read event log grouped by skill name.
	// It is preferred over LogPath so that callers listing many skills can
	// read the log once instead of once per skill (see #221). Entries
	// whose skill key matches Collect's skillName argument are used; the
	// rest are ignored.
	Events map[string][]events.Event
	// InstallOpt locates installed copies for the marker fallback and the
	// last-used probe. When both HomeDir and ProjectDir are empty the
	// install scan is skipped entirely, keeping callers that do not know a
	// home (e.g. MCP servers configured without one) hermetic.
	InstallOpt install.Options
}

// UsageProbe is an opt-in, best-effort source of last-used evidence for an
// installed skill. Probes are registered globally before Collect runs and
// must be conservative: returning false is always preferred over a guessed
// timestamp. Name is reported verbatim as last_used_source. No probe is
// registered by default — symskills never fabricates usage data.
type UsageProbe struct {
	Name   string
	Lookup func(installPath string) (time.Time, bool)
}

var usageProbes []UsageProbe

// RegisterUsageProbe adds an opt-in last-used adapter, e.g. one reading a
// harness's own session or skill log where such a log exists and is
// documented. See README "Per-Skill Metadata" for the contract.
func RegisterUsageProbe(p UsageProbe) {
	usageProbes = append(usageProbes, p)
}

// resetUsageProbes clears registered probes; used by tests.
func resetUsageProbes() {
	usageProbes = nil
}

// Collect assembles the metadata record for the skill at root. It never
// fails: every evidence source is optional and every error degrades to an
// empty field. skillName keys the event-log filter and the install scan.
func Collect(root, skillName string, opts Options) Record {
	rec := Record{}
	rec.CreatedAt, rec.ModifiedAt = timesFromFS(root)

	installs := map[string]Install{} // by target
	var lastRendered time.Time
	for _, ev := range readEvents(opts.LogPath, skillName, opts.Events) {
		ts, err := time.Parse(time.RFC3339Nano, ev.TS)
		if err != nil || ev.Outcome != events.OutcomeOK {
			continue
		}
		switch ev.Event {
		case events.EventRender:
			if ts.After(lastRendered) {
				lastRendered = ts
			}
		case events.EventInstall, events.EventProfileInstall:
			if ev.Target == "" {
				continue
			}
			if cur, ok := installs[ev.Target]; !ok || ts.After(parseTS(cur.InstalledAt)) {
				installs[ev.Target] = Install{Target: ev.Target, Path: ev.Path, InstalledAt: ev.TS}
			}
		}
	}

	// Marker fallback: fills targets the event log does not cover (e.g. a
	// library imported before the log existed) and supplies a render time
	// when no render event has been recorded.
	collectMarkers(skillName, opts, installs, &lastRendered)

	if !lastRendered.IsZero() {
		rec.LastRenderedAt = lastRendered.UTC().Format(time.RFC3339)
	}
	for _, target := range sortedKeys(installs) {
		rec.Installs = append(rec.Installs, installs[target])
	}
	if rec.Installs == nil {
		rec.Installs = []Install{}
	}

	if used, source := lastUsed(rec.Installs); source != "" {
		rec.LastUsed = &used
		rec.LastUsedSource = source
	}
	return rec
}

// timesFromFS derives created (directory mtime) and modified (newest file
// mtime in the tree, .git excluded) from the filesystem. Both degrade to ""
// when the directory is unreadable.
func timesFromFS(root string) (created, modified string) {
	if st, err := os.Stat(root); err == nil {
		created = st.ModTime().UTC().Format(time.RFC3339)
	}
	if m, err := newestMtime(root); err == nil && !m.IsZero() {
		modified = m.UTC().Format(time.RFC3339)
	} else if st, err := os.Stat(filepath.Join(root, "SKILL.md")); err == nil {
		modified = st.ModTime().UTC().Format(time.RFC3339)
	}
	return created, modified
}

// newestMtime returns the newest modification time among all regular files
// under root, skipping .git directories.
func newestMtime(root string) (time.Time, error) {
	var best time.Time
	err := filepath.WalkDir(root, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() {
			if d.Name() == ".git" {
				return filepath.SkipDir
			}
			return nil
		}
		if info, err := d.Info(); err == nil && info.ModTime().After(best) {
			best = info.ModTime()
		}
		return nil
	})
	if err != nil {
		return time.Time{}, err
	}
	return best, nil
}

// readEvents returns the matching records from the lifecycle log, or nil
// when the log is absent or unreadable. Log failures never fail Collect.
// When opts.Events is populated, it is used instead of reading the file,
// so that a caller listing many skills reads the log once (see #221).
func readEvents(logPath, skillName string, preRead map[string][]events.Event) []events.Event {
	if preRead != nil {
		recs := preRead[skillName]
		return recs
	}
	if logPath == "" {
		return nil
	}
	recs, err := events.New(logPath, "").Read(events.Filter{Skill: skillName})
	if err != nil {
		return nil
	}
	return recs
}

// ReadEventsLog reads the entire lifecycle log once and groups records by
// skill name. Callers that invoke Collect for many skills should call this
// once and pass the result via Options.Events to avoid re-reading the log
// for every skill (see #221). Records with an empty Skill field are
// included under the empty-string key.
func ReadEventsLog(logPath string) (map[string][]events.Event, error) {
	if logPath == "" {
		return nil, nil
	}
	recs, err := events.New(logPath, "").Read(events.Filter{})
	if err != nil {
		return nil, err
	}
	grouped := make(map[string][]events.Event)
	for _, ev := range recs {
		grouped[ev.Skill] = append(grouped[ev.Skill], ev)
	}
	return grouped, nil
}

// collectMarkers scans each registered target's install path for a
// .symskills.json marker. Targets already covered by event-log records are
// skipped; marker-only targets are added to installs. When no render event
// exists, the marker's rendered-tree mtime (falling back to its installed
// timestamp) supplies lastRendered.
func collectMarkers(skillName string, opts Options, installs map[string]Install, lastRendered *time.Time) {
	if opts.InstallOpt.HomeDir == "" && opts.InstallOpt.ProjectDir == "" {
		return
	}
	for _, target := range render.DefaultTargets() {
		t := string(target)
		if _, covered := installs[t]; covered {
			continue
		}
		dest, err := install.InstallPath(target, skillName, opts.InstallOpt)
		if err != nil {
			continue
		}
		m, ok := readMarker(dest)
		if !ok {
			continue
		}
		installs[t] = Install{Target: t, Path: dest, InstalledAt: m.Installed}
		if lastRendered.IsZero() {
			if st, err := os.Stat(m.RenderedAt); err == nil {
				*lastRendered = st.ModTime()
			} else if ts := parseTS(m.Installed); !ts.IsZero() {
				*lastRendered = ts
			}
		}
	}
}

// marker is the subset of the install marker symskills reads. Fields follow
// docs/marker-protocol.md.
type marker struct {
	Target     string `json:"target"`
	Name       string `json:"name"`
	Installed  string `json:"installed"`
	RenderedAt string `json:"rendered_at"`
}

// readMarker reads the install marker at dest. A marker without an
// installed timestamp is not evidence.
func readMarker(dest string) (marker, bool) {
	data, err := os.ReadFile(filepath.Join(dest, markerFile))
	if err != nil {
		return marker{}, false
	}
	var m marker
	if err := json.Unmarshal(data, &m); err != nil {
		return marker{}, false
	}
	if m.Installed == "" && m.RenderedAt == "" {
		return marker{}, false
	}
	return m, true
}

// minUsageGap excludes reads that happened as part of the install
// operation itself: symskills' own bookkeeping touches the installed copy
// at install time (marker writes, path resolution), and that must never be
// reported as harness usage. A harness reading the skill later is captured
// regardless of the gap, because the first read after the install write
// updates atime on relatime-style mounts.
const minUsageGap = time.Minute

// lastUsed returns the strongest last-used evidence across all installed
// copies: the installed SKILL.md's access time where the filesystem records
// one usefully, and any registered opt-in usage probe. The source name is
// returned alongside so callers can report where the signal came from.
func lastUsed(installs []Install) (time.Time, string) {
	var best time.Time
	var bestSource string
	for _, inst := range installs {
		if inst.Path == "" {
			continue
		}
		// Only the skill file itself is probed, never the directory: a
		// directory's atime is bumped by mere path resolution (including
		// our own marker reads) and is not usage evidence.
		used, ok := usefulAccessTime(filepath.Join(inst.Path, "SKILL.md"), parseTS(inst.InstalledAt))
		if ok && used.After(best) {
			best, bestSource = used, "install_atime"
		}
		for _, probe := range usageProbes {
			if used, ok := probe.Lookup(inst.Path); ok && used.After(best) {
				best, bestSource = used, probe.Name
			}
		}
	}
	return best, bestSource
}

// usefulAccessTime returns the file's access time only when it records a
// read that happened after the file was last written (atime > mtime) and
// after the install operation itself (atime >= installedAt + minUsageGap).
// On relatime/noatime mounts atime is not a reliable usage signal, so every
// other case reports nothing rather than a fabricated timestamp.
func usefulAccessTime(path string, installedAt time.Time) (time.Time, bool) {
	fi, err := os.Stat(path)
	if err != nil {
		return time.Time{}, false
	}
	at := accessTime(fi)
	if at.IsZero() || !at.After(fi.ModTime()) {
		return time.Time{}, false
	}
	if !installedAt.IsZero() && at.Before(installedAt.Add(minUsageGap)) {
		return time.Time{}, false
	}
	return at, true
}

// parseTS parses an RFC3339 timestamp, returning the zero time on failure.
func parseTS(s string) time.Time {
	ts, err := time.Parse(time.RFC3339Nano, s)
	if err != nil {
		return time.Time{}
	}
	return ts
}

// sortedKeys returns the map keys in sorted order for deterministic output.
func sortedKeys[V any](m map[string]V) []string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return keys
}
