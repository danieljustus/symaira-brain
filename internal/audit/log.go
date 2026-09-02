package audit

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/danieljustus/symaira-brain/internal/xdg"
	"github.com/danieljustus/symaira-corekit/auditkit"
)

// Entry is one JSONL audit record for a routed tool call.
type Entry struct {
	Timestamp  string `json:"timestamp"`
	SessionID  string `json:"session_id,omitempty"`
	Profile    string `json:"profile"`
	Server     string `json:"server"`
	Tool       string `json:"tool"`
	DurationMS int64  `json:"duration_ms"`
	Status     string `json:"status"`
	Category   string `json:"category,omitempty"`
	Retryable  bool   `json:"retryable"`
	Reason     string `json:"reason,omitempty"`
	Level      string `json:"level,omitempty"`
	ArgKeys    string `json:"arg_keys,omitempty"`
	ArgValues  string `json:"arg_values,omitempty"`
	// AccessClass and AccessSource explain a foreign-server tool call's
	// exposure decision (read/write class and what it was derived from).
	// Empty for the four cores.
	AccessClass  string `json:"access_class,omitempty"`
	AccessSource string `json:"access_source,omitempty"`
}

// Exposure carries a foreign-server tool call's access classification into
// the audit entry so an exposure decision can be explained after the fact.
// The zero value is a core-server call (no foreign exposure semantics).
type Exposure struct {
	AccessClass  string `json:"access_class,omitempty"`
	AccessSource string `json:"access_source,omitempty"`
}

// Degradation records a backend that was unavailable while a gateway session
// built its catalog.
type Degradation struct {
	SessionID string `json:"session_id,omitempty"`
	Profile   string `json:"profile,omitempty"`
	Server    string `json:"server"`
	Reason    string `json:"reason"`
	Level     string `json:"level"`
}

// Classification describes why a routed call failed and whether retrying it
// unchanged is expected to help.
type Classification struct {
	Category  string `json:"category"`
	Retryable bool   `json:"retryable"`
}

// Config controls audit logging behavior.
type Config struct {
	Enabled bool
	Verbose bool
}

// Logger writes JSONL audit entries with strict redaction. Entries are
// persisted through corekit/auditkit's tamper-evident Sink (hash-chained,
// rotatable), so the log gains chain verification and anchor checkpoints
// without changing this package's API. It is safe for concurrent use.
type Logger struct {
	mu       sync.Mutex
	sink     *auditkit.Sink
	path     string
	profile  string
	session  string
	config   Config
	degraded bool  // true after any write failure
	dropped  int64 // total entries dropped due to write failures
}

// maxFileSize is the size threshold for log rotation (10 MB). The auditkit
// sink uses the same default; kept here for the config surface.

// Open creates or opens the audit log file for the given profile. If
// audit is disabled in config, returns a no-op logger.
func Open(profile string, cfg Config) (*Logger, error) {
	if !cfg.Enabled {
		return &Logger{config: cfg}, nil
	}

	dir, err := xdg.AuditDir()
	if err != nil {
		return nil, fmt.Errorf("audit: resolve audit dir: %w", err)
	}
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return nil, fmt.Errorf("audit: create audit dir: %w", err)
	}

	path := filepath.Join(dir, profile+".jsonl")
	sink, err := auditkit.OpenSink(path)
	if err != nil {
		return nil, fmt.Errorf("audit: open %s: %w", path, err)
	}

	return &Logger{
		sink:    sink,
		path:    path,
		profile: profile,
		session: time.Now().UTC().Format(time.RFC3339Nano),
		config:  cfg,
	}, nil
}

// Log records a tool call. server is "vault", "memory", or "skills" (or a
// foreign server alias). args are the raw JSON arguments (may be nil).
// duration is the call wall-clock time. status is "ok", "error", or
// "timeout". exposure records the foreign-server access classification
// (zero value for the four cores). An optional classification records the
// reason for a failed call.
func (l *Logger) Log(server, tool string, args json.RawMessage, duration time.Duration, status string, exposure Exposure, classifications ...Classification) {
	if l == nil || l.sink == nil || !l.config.Enabled {
		return
	}

	entry := Entry{
		Timestamp:    time.Now().UTC().Format(time.RFC3339Nano),
		SessionID:    l.session,
		Profile:      l.profile,
		Server:       server,
		Tool:         tool,
		DurationMS:   duration.Milliseconds(),
		Status:       status,
		AccessClass:  exposure.AccessClass,
		AccessSource: exposure.AccessSource,
	}
	if len(classifications) > 0 {
		entry.Category = classifications[0].Category
		entry.Retryable = classifications[0].Retryable
	}

	entry.ArgKeys, entry.ArgValues = redactArgs(server, tool, args, l.config.Verbose)

	l.mu.Lock()
	defer l.mu.Unlock()

	data, err := json.Marshal(entry)
	if err != nil {
		l.degraded = true
		l.dropped++
		return
	}
	if err := l.sink.Append(string(data)); err != nil {
		l.degraded = true
		l.dropped++
	}
}

// LogDegradation records a backend omitted from the catalog during startup.
func (l *Logger) LogDegradation(server, reason, level string) {
	if l == nil || l.sink == nil || !l.config.Enabled {
		return
	}

	entry := Entry{
		Timestamp: time.Now().UTC().Format(time.RFC3339Nano),
		SessionID: l.session,
		Profile:   l.profile,
		Server:    server,
		Status:    "degraded",
		Reason:    reason,
		Level:     level,
	}
	data, err := json.Marshal(entry)
	if err != nil {
		l.mu.Lock()
		l.degraded = true
		l.dropped++
		l.mu.Unlock()
		return
	}

	l.mu.Lock()
	defer l.mu.Unlock()
	if err := l.sink.Append(string(data)); err != nil {
		l.degraded = true
		l.dropped++
	}
}

// maxArgValueLen is the maximum length of a logged argument value in
// verbose mode. Values longer than this are truncated to prevent the
// audit log from growing unboundedly with user content.
const maxArgValueLen = 256

// sensitiveKeys contains argument key names (lowercased) whose values
// may carry credentials or other sensitive data. These are always
// redacted in verbose mode regardless of server, to avoid logging
// secrets verbatim. The set covers common case variants and separator
// variants (underscore, hyphen, concatenated).
var sensitiveKeys = map[string]bool{
	// password
	"password": true, "passwd": true, "pass": true, "pwd": true,
	// token
	"token": true, "access_token": true, "access-token": true, "accesstoken": true,
	"refresh_token": true, "refresh-token": true, "refreshtoken": true,
	// secret
	"secret": true, "client_secret": true, "client-secret": true, "clientsecret": true,
	// api key
	"api_key": true, "api-key": true, "apikey": true,
	// authorization / auth
	"authorization": true, "auth": true, "bearer": true,
	// credential
	"credential": true, "credentials": true,
	// private key
	"private_key": true, "private-key": true, "privatekey": true,
	// passphrase
	"passphrase": true,
	// content (existing behavior)
	"content": true,
}

// isSensitiveKey reports whether key should be treated as sensitive,
// matching case-insensitively.
func isSensitiveKey(key string) bool {
	k := strings.ToLower(key)
	return sensitiveKeys[k]
}

// redactArgs applies the redaction policy:
//   - vault_* tools: never log arguments or values in any mode
//   - other servers: log argument KEYS only by default;
//     verbose=true logs values too (still never for vault).
//     Sensitive fields are recursively redacted with "[redacted]".
//     Values are capped at maxArgValueLen characters.
func redactArgs(server, tool string, args json.RawMessage, verbose bool) (keys, values string) {
	if len(args) == 0 {
		return "", ""
	}

	isVault := server == "vault" || strings.HasPrefix(tool, "vault_")
	if isVault {
		return "", ""
	}

	var m map[string]any
	if err := json.Unmarshal(args, &m); err != nil {
		return "", ""
	}

	redacted := redactMap(m)

	keyList := make([]string, 0, len(m))
	for k := range m {
		keyList = append(keyList, k)
	}
	keys = strings.Join(keyList, ",")

	if verbose {
		valParts := make([]string, 0, len(keyList))
		for _, k := range keyList {
			v := fmt.Sprintf("%v", redacted[k])
			if len(v) > maxArgValueLen {
				v = v[:maxArgValueLen] + "…"
			}
			valParts = append(valParts, fmt.Sprintf("%s=%s", k, v))
		}
		values = strings.Join(valParts, ",")
	}

	return keys, values
}

// redactMap returns a copy of m with sensitive values replaced by "[redacted]",
// recursing into nested maps and slices.
func redactMap(m map[string]any) map[string]any {
	result := make(map[string]any, len(m))
	for k, v := range m {
		if isSensitiveKey(k) {
			result[k] = "[redacted]"
		} else {
			result[k] = redactValue(v)
		}
	}
	return result
}

// redactValue recurses into maps and slices, returning a copy with any
// sensitive nested values replaced by "[redacted]".
func redactValue(v any) any {
	switch val := v.(type) {
	case map[string]any:
		return redactMap(val)
	case []any:
		result := make([]any, len(val))
		for i, item := range val {
			result[i] = redactValue(item)
		}
		return result
	default:
		return v
	}
}

// Close closes the underlying file. The logger is unusable afterward.
func (l *Logger) Close() error {
	if l == nil || l.sink == nil {
		return nil
	}
	l.mu.Lock()
	defer l.mu.Unlock()
	return l.sink.Close()
}

// Degraded reports whether the logger has encountered a write or rotation
// failure since it was opened. The caller can use this to emit a warning
// without exposing entry contents or argument values.
func (l *Logger) Degraded() bool {
	if l == nil {
		return false
	}
	l.mu.Lock()
	defer l.mu.Unlock()
	return l.degraded
}

// LatestDegradations returns degradation records from the most recent session
// in each matching profile log. An empty profile reads every profile log.
// It uses a bounded reverse reader to avoid loading the entire file into
// memory for the common path.
func LatestDegradations(profile string) ([]Degradation, error) {
	dir, err := xdg.AuditDir()
	if err != nil {
		return nil, fmt.Errorf("audit: resolve audit dir: %w", err)
	}
	pattern := filepath.Join(dir, "*.jsonl")
	if profile != "" {
		pattern = filepath.Join(dir, profile+".jsonl")
	}
	paths, err := filepath.Glob(pattern)
	if err != nil {
		return nil, fmt.Errorf("audit: find logs: %w", err)
	}

	var result []Degradation
	for _, path := range paths {
		entries, err := tailEntriesBounded(path, 0, func(entry Entry) bool {
			return entry.Status == "degraded"
		}, true)
		if err != nil {
			return nil, fmt.Errorf("audit: read %s: %w", path, err)
		}

		for _, entry := range entries {
			result = append(result, Degradation{
				SessionID: entry.SessionID,
				Profile:   entry.Profile,
				Server:    entry.Server,
				Reason:    entry.Reason,
				Level:     entry.Level,
			})
		}
	}
	return result, nil
}

// tailEntriesBounded reads entries from a JSONL file backwards in chunks,
// stopping after limit matching entries are collected (0 = no limit) or,
// when sessionScoped is true, after the latest session is complete. It
// never loads the entire file into memory for the common path.
//
// The filter predicate, when non-nil, is applied to each entry; only
// entries for which it returns true are counted toward limit and
// returned. A nil filter matches every entry. Results are returned in
// chronological order (oldest first).
func tailEntriesBounded(path string, limit int, filter func(Entry) bool, sessionScoped bool) ([]Entry, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()

	info, err := f.Stat()
	if err != nil {
		return nil, err
	}
	size := info.Size()
	if size == 0 {
		return nil, nil
	}

	var carry []byte // prefix of a line that continues into the next chunk
	var latestSession string
	var results []Entry // collected in reverse chronological order
	consume := func(line []byte) bool {
		if len(line) == 0 {
			return false
		}

		var entry Entry
		if err := json.Unmarshal(line, &entry); err != nil {
			return false
		}

		if sessionScoped {
			if latestSession == "" {
				if entry.SessionID == "" {
					return false
				}
				latestSession = entry.SessionID
			}

			if entry.SessionID != latestSession {
				return true
			}
		}
		if filter == nil || filter(entry) {
			results = append(results, entry)
			return limit > 0 && len(results) >= limit
		}
		return false
	}

	offset := size
	for offset > 0 {
		readSize := int64(tailChunkSize)
		if offset < readSize {
			readSize = offset
		}
		offset -= readSize

		chunk := make([]byte, int(readSize))
		if _, err := f.ReadAt(chunk, offset); err != nil {
			return nil, err
		}

		// The first bytes of the later chunk may be a line fragment. Append
		// that fragment to this chunk so the line is decoded exactly once.
		if len(carry) > 0 {
			chunk = append(chunk, carry...)
			carry = nil
		}

		// Process lines in reverse order (most recent first).
		lineEnd := len(chunk)
		for i := len(chunk) - 1; i >= 0; i-- {
			if chunk[i] != '\n' {
				continue
			}
			if consume(chunk[i+1 : lineEnd]) {
				reverseEntries(results)
				return results, nil
			}
			lineEnd = i
		}

		// Keep the prefix before the earliest newline for the preceding
		// chunk. At offset zero it is the complete first line.
		if lineEnd > 0 {
			carry = append([]byte(nil), chunk[:lineEnd]...)
		}
	}

	if len(carry) > 0 && consume(carry) {
		// The limit was reached while processing the earliest line.
		reverseEntries(results)
		return results, nil
	}

	reverseEntries(results)
	return results, nil
}

func reverseEntries(entries []Entry) {
	for i, j := 0, len(entries)-1; i < j; i, j = i+1, j-1 {
		entries[i], entries[j] = entries[j], entries[i]
	}
}

const tailChunkSize = 64 * 1024 // 64KB

// auditLogPaths resolves the JSONL audit log paths to read for profile.
// If profile is empty, it returns every profile log found in the audit
// directory. This discovery is shared by Tail (via TailEntries) and
// TailEntries so both read the same set of files for the same inputs.
func auditLogPaths(profile string) ([]string, error) {
	dir, err := xdg.AuditDir()
	if err != nil {
		return nil, fmt.Errorf("audit: resolve audit dir: %w", err)
	}

	if profile != "" {
		return []string{filepath.Join(dir, profile+".jsonl")}, nil
	}

	entries, err := os.ReadDir(dir)
	if err != nil {
		return nil, fmt.Errorf("audit: read audit dir: %w", err)
	}
	var paths []string
	for _, e := range entries {
		if !e.IsDir() && strings.HasSuffix(e.Name(), ".jsonl") {
			paths = append(paths, filepath.Join(dir, e.Name()))
		}
	}
	return paths, nil
}

// Tail reads the last n entries from the audit log for the given profile
// and writes them human-readably to w. If profile is empty, uses all
// profiles found in the audit directory.
func Tail(w io.Writer, profile string, n int) error {
	entries, err := TailEntries(profile, n)
	if err != nil {
		return err
	}
	for _, entry := range entries {
		printEntry(w, entry)
	}
	return nil
}

// TailEntries reads the last n entries from the audit log for the given
// profile and returns them as a slice. If profile is empty, merges
// entries from all profiles found in the audit directory.
func TailEntries(profile string, n int) ([]Entry, error) {
	paths, err := auditLogPaths(profile)
	if err != nil {
		return nil, err
	}

	var result []Entry
	for _, path := range paths {
		entries, err := tailEntriesBounded(path, n, nil, false)
		if err != nil {
			continue
		}
		prof := strings.TrimSuffix(filepath.Base(path), ".jsonl")
		for _, entry := range entries {
			entry.Profile = prof
			result = append(result, entry)
		}
	}

	return result, nil
}

func printEntry(w io.Writer, e Entry) {
	ts, _ := time.Parse(time.RFC3339Nano, e.Timestamp)
	local := ts.Local().Format("2006-01-02 15:04:05")

	extra := ""
	if e.ArgKeys != "" {
		extra = fmt.Sprintf(" keys=%s", e.ArgKeys)
	}

	fmt.Fprintf(w, "%s  %-10s  %-8s  %-25s  %dms  %s%s\n",
		local, e.Profile, e.Server, e.Tool, e.DurationMS, e.Status, extra)
}
