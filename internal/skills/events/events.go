// Package events provides the local append-only operation log for symskills.
//
// Every skill-mutating operation (import, render, install, uninstall, profile
// install, validate failure) appends one JSONL record to
// ~/.local/share/symskills/events.jsonl. The log is local-only: it is not
// telemetry and is never transmitted anywhere. The same write path is shared
// by the CLI, the MCP tools and (through them) the macOS client, so all
// actors produce identical records distinguished only by the actor field.
//
// The log is best-effort by design: a failure to write (unwritable location,
// disk full, ...) is swallowed and never fails the operation being logged.
package events

import (
	"bytes"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"
)

// Event names for skill-mutating operations.
const (
	EventImport          = "import"
	EventRender          = "render"
	EventInstall         = "install"
	EventUninstall       = "uninstall"
	EventProfileInstall  = "profile_install"
	EventValidateFailure = "validate_failure"
)

// Actor identifies which caller performed the operation.
const (
	ActorCLI    = "cli"
	ActorMCP    = "mcp"
	ActorClient = "client"
)

// Outcome of the logged operation.
const (
	OutcomeOK    = "ok"
	OutcomeError = "error"
)

// DefaultMaxBytes bounds the current log file before it rotates to
// events.1.jsonl. The file can therefore never grow without bound: it stays
// below DefaultMaxBytes plus one record, and at most one previous file is
// kept.
const DefaultMaxBytes = 1 << 20 // 1 MiB

// Event is one operation record. All JSON fields are snake_case per the
// repo-wide payload convention. Optional fields are omitted when empty.
type Event struct {
	TS           string `json:"ts"`                      // RFC3339 UTC
	Event        string `json:"event"`                   // one of the Event* constants
	Skill        string `json:"skill,omitempty"`         // skill name
	SkillVersion string `json:"skill_version,omitempty"` // version from the source frontmatter
	Target       string `json:"target,omitempty"`        // harness target (or "all")
	Scope        string `json:"scope,omitempty"`         // user | project
	Mode         string `json:"mode,omitempty"`          // symlink | copy
	Path         string `json:"path,omitempty"`          // affected path
	SourceHash   string `json:"source_hash,omitempty"`   // reserved; not populated yet
	Outcome      string `json:"outcome"`                 // ok | error
	Error        string `json:"error,omitempty"`         // message when outcome is error
	ToolVersion  string `json:"tool_version,omitempty"`  // symskills build version
	Actor        string `json:"actor"`                   // cli | mcp | client
}

// Filter narrows which records Read returns. Empty fields match everything.
type Filter struct {
	Skill  string
	Target string
	// Since is an inclusive lower bound on ts. Zero means no bound.
	Since time.Time
}

// Matches reports whether ev satisfies the filter.
func (f Filter) Matches(ev Event) bool {
	if f.Skill != "" && ev.Skill != f.Skill {
		return false
	}
	if f.Target != "" && ev.Target != f.Target {
		return false
	}
	if !f.Since.IsZero() {
		ts, err := time.Parse(time.RFC3339Nano, ev.TS)
		if err != nil || ts.Before(f.Since) {
			return false
		}
	}
	return true
}

// Logger appends Event records to a single JSONL file with size-bounded
// rotation. A nil *Logger is a valid no-op logger: Record and Read behave as
// if logging were disabled. This keeps call sites that may run without a
// configured log location (e.g. MCP servers in tests) free of nil checks.
type Logger struct {
	mu       sync.Mutex
	path     string
	version  string
	maxBytes int64
}

// New returns a Logger writing to path with the default rotation limit.
// toolVersion is stamped into every record that does not set its own.
func New(path, toolVersion string) *Logger {
	return NewWithLimit(path, toolVersion, DefaultMaxBytes)
}

// NewWithLimit returns a Logger that rotates the current file to
// events.1.jsonl once appending a record would exceed maxBytes.
func NewWithLimit(path, toolVersion string, maxBytes int64) *Logger {
	if maxBytes <= 0 {
		maxBytes = DefaultMaxBytes
	}
	return &Logger{path: path, version: toolVersion, maxBytes: maxBytes}
}

// Path returns the current log file location.
func (l *Logger) Path() string {
	if l == nil {
		return ""
	}
	return l.path
}

// Record appends one event to the log. The timestamp is stamped (RFC3339,
// UTC) when ev.TS is empty, and the logger's tool version fills an empty
// ToolVersion. Record never returns an error: logging is best-effort and a
// log failure must not fail the operation being logged. The empty actor
// defaults to ActorCLI.
func (l *Logger) Record(ev Event) {
	if l == nil {
		return
	}
	l.mu.Lock()
	defer l.mu.Unlock()
	if ev.TS == "" {
		ev.TS = time.Now().UTC().Format(time.RFC3339)
	}
	if ev.ToolVersion == "" {
		ev.ToolVersion = l.version
	}
	if ev.Actor == "" {
		ev.Actor = ActorCLI
	}
	data, err := json.Marshal(ev)
	if err != nil {
		return // fields are all JSON-safe; cannot happen in practice
	}
	l.append(append(data, '\n'))
}

// Read returns all records matching filter in chronological order: the
// rotated events.1.jsonl (older) first, then the current events.jsonl.
// Malformed lines are skipped so a torn write can never break the log
// command. A nil Logger reads nothing.
func (l *Logger) Read(filter Filter) ([]Event, error) {
	if l == nil {
		return nil, nil
	}
	l.mu.Lock()
	defer l.mu.Unlock()
	var out []Event
	for _, path := range []string{rotatedPath(l.path), l.path} {
		data, err := os.ReadFile(path)
		if errors.Is(err, os.ErrNotExist) {
			continue
		}
		if err != nil {
			return nil, err
		}
		for _, line := range bytes.Split(data, []byte("\n")) {
			if len(bytes.TrimSpace(line)) == 0 {
				continue
			}
			var ev Event
			if err := json.Unmarshal(line, &ev); err != nil {
				continue // skip torn or corrupt lines
			}
			if filter.Matches(ev) {
				out = append(out, ev)
			}
		}
	}
	return out, nil
}

// append writes one JSON line, rotating first when the current file is at
// its size limit. All errors are swallowed: the operation being logged must
// proceed regardless.
func (l *Logger) append(line []byte) {
	if err := l.rotateIfNeeded(int64(len(line))); err != nil {
		// Degrade: still try to append to the current file.
	}
	if err := os.MkdirAll(filepath.Dir(l.path), 0o755); err != nil {
		return
	}
	f, err := os.OpenFile(l.path, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o644)
	if err != nil {
		return
	}
	defer f.Close()
	_, _ = f.Write(line)
}

// rotateIfNeeded moves the current file to events.1.jsonl (dropping any
// older rotated file) when appending lineLen more bytes would exceed the
// rotation limit. The rotated file is kept, so at least one previous file
// survives every rotation.
func (l *Logger) rotateIfNeeded(lineLen int64) error {
	st, err := os.Stat(l.path)
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	if st.Size()+lineLen <= l.maxBytes {
		return nil
	}
	prev := rotatedPath(l.path)
	if err := os.Remove(prev); err != nil && !errors.Is(err, os.ErrNotExist) {
		return err
	}
	return os.Rename(l.path, prev)
}

// rotatedPath returns the name of the previous-generation file for path
// (events.jsonl -> events.1.jsonl).
func rotatedPath(path string) string {
	ext := filepath.Ext(path)
	return strings.TrimSuffix(path, ext) + ".1" + ext
}

// DefaultPath returns the standard log location under the current home
// directory, honoring $HOME for tests and sandboxed runs.
func DefaultPath() string {
	home, err := os.UserHomeDir()
	if err != nil || home == "" {
		home = "."
	}
	return filepath.Join(home, ".local", "share", "symskills", "events.jsonl")
}
