package audit

import (
	"bufio"
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
		})
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

// tailEntries reads up to n entries (0 = all) from a JSONL file using a
// buffered scanner with a 1 MB token limit to avoid silent truncation
// on very long lines.
func tailEntries(path string, n int) ([]Entry, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()

	var entries []Entry
	scanner := bufio.NewScanner(f)
	scanner.Buffer(make([]byte, 0, 64*1024), 1<<20)
	for scanner.Scan() {
		var entry Entry
		if err := json.Unmarshal(scanner.Bytes(), &entry); err != nil {
			continue
		}
		entries = append(entries, entry)
	}
	if err := scanner.Err(); err != nil {
		return nil, err
	}
	if n > 0 && len(entries) > n {
		entries = entries[len(entries)-n:]
	}
	return entries, nil
}

// tailEntriesBounded reads entries from a JSONL file backwards in chunks,
// stopping after the latest session is complete or after limit matching
// entries are collected (0 = no limit). It never loads the entire file
// into memory for the common path.
//
// The filter predicate is applied to each entry; only entries for which
// filter returns true are returned. Results are returned in chronological
// order (oldest first).
func tailEntriesBounded(path string, limit int, filter func(Entry) bool) ([]Entry, error) {
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

	const chunkSize = 64 * 1024
	var incomplete []byte // trailing partial line from the previous (later) chunk

	var latestSession string
	var results []Entry // collected in reverse chronological order

	offset := size
	for offset > 0 {
		readSize := chunkSize
		if offset < int64(readSize) {
			readSize = int(offset)
		}
		offset -= int64(readSize)

		chunk := make([]byte, readSize)
		if _, err := f.ReadAt(chunk, offset); err != nil {
			return nil, err
		}

		// Prepend any incomplete line from the next (later) chunk.
		if len(incomplete) > 0 {
			chunk = append(incomplete, chunk...)
			incomplete = nil
		}

		// Split on newlines. The last element is incomplete if the chunk
		// (after prepending) does not end with '\n'.
		var lines [][]byte
		start := 0
		for i := 0; i < len(chunk); i++ {
			if chunk[i] == '\n' {
				lines = append(lines, chunk[start:i])
				start = i + 1
			}
		}
		if start < len(chunk) {
			incomplete = chunk[start:]
		}

		// Process lines in reverse order (most recent first).
		for i := len(lines) - 1; i >= 0; i-- {
			line := lines[i]
			if len(line) == 0 {
				continue
			}

			var entry Entry
			if err := json.Unmarshal(line, &entry); err != nil {
				continue
			}

			if latestSession == "" {
				if entry.SessionID != "" {
					latestSession = entry.SessionID
				} else {
					continue
				}
			}

			if entry.SessionID != latestSession {
				// Completed the latest session; return in chronological order.
				reverseEntries(results)
				return results, nil
			}

			if filter == nil || filter(entry) {
				results = append(results, entry)
				if limit > 0 && len(results) >= limit {
					reverseEntries(results)
					return results, nil
				}
			}
		}
	}

	// Process any remaining incomplete line (only possible for the first
	// chunk when the file does not end with '\n').
	if len(incomplete) > 0 {
		var entry Entry
		if err := json.Unmarshal(incomplete, &entry); err == nil {
			if latestSession == "" {
				if entry.SessionID != "" {
					latestSession = entry.SessionID
				}
			} else if entry.SessionID == latestSession {
				if filter == nil || filter(entry) {
					results = append(results, entry)
				}
			}
		}
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

// Tail reads the last n entries from the audit log for the given profile
// and writes them human-readably to w. If profile is empty, uses all
// profiles found in the audit directory.
func Tail(w io.Writer, profile string, n int) error {
	dir, err := xdg.AuditDir()
	if err != nil {
		return fmt.Errorf("audit: resolve audit dir: %w", err)
	}

	var paths []string
	if profile != "" {
		paths = []string{filepath.Join(dir, profile+".jsonl")}
	} else {
		entries, err := os.ReadDir(dir)
		if err != nil {
			return fmt.Errorf("audit: read audit dir: %w", err)
		}
		for _, e := range entries {
			if !e.IsDir() && strings.HasSuffix(e.Name(), ".jsonl") {
				paths = append(paths, filepath.Join(dir, e.Name()))
			}
		}
	}

	for _, path := range paths {
		lines, err := tailFile(path, n)
		if err != nil {
			continue
		}
		prof := strings.TrimSuffix(filepath.Base(path), ".jsonl")
		for _, line := range lines {
			var entry Entry
			if err := json.Unmarshal([]byte(line), &entry); err != nil {
				continue
			}
			entry.Profile = prof
			printEntry(w, entry)
		}
	}

	return nil
}

// TailEntries reads the last n entries from the audit log for the given
// profile and returns them as a slice. If profile is empty, merges
// entries from all profiles found in the audit directory.
func TailEntries(profile string, n int) ([]Entry, error) {
	dir, err := xdg.AuditDir()
	if err != nil {
		return nil, fmt.Errorf("audit: resolve audit dir: %w", err)
	}

	var paths []string
	if profile != "" {
		paths = []string{filepath.Join(dir, profile+".jsonl")}
	} else {
		entries, err := os.ReadDir(dir)
		if err != nil {
			return nil, fmt.Errorf("audit: read audit dir: %w", err)
		}
		for _, e := range entries {
			if !e.IsDir() && strings.HasSuffix(e.Name(), ".jsonl") {
				paths = append(paths, filepath.Join(dir, e.Name()))
			}
		}
	}

	var result []Entry
	for _, path := range paths {
		lines, err := tailFile(path, n)
		if err != nil {
			continue
		}
		prof := strings.TrimSuffix(filepath.Base(path), ".jsonl")
		for _, line := range lines {
			var entry Entry
			if err := json.Unmarshal([]byte(line), &entry); err != nil {
				continue
			}
			entry.Profile = prof
			result = append(result, entry)
		}
	}

	return result, nil
}

// tailFile reads the last n lines from a JSONL file. For small files
// (at most tailChunkSize) it reads the whole file at once; for larger
// files it scans backwards from the end in bounded chunks, counting
// newlines until enough lines are collected.
func tailFile(path string, n int) ([]string, error) {
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

	// Small files: read whole file.
	if size <= tailChunkSize {
		data := make([]byte, size)
		if _, err := f.ReadAt(data, 0); err != nil {
			return nil, err
		}
		return splitTailLines(data, n)
	}

	// Large files: scan backwards from the end.
	buf := make([]byte, tailChunkSize)
	newlinesFound := 0
	cutoff := int64(0)
	offset := size

scan:
	for offset > 0 {
		readSize := int64(tailChunkSize)
		if offset < readSize {
			readSize = offset
		}
		offset -= readSize

		if _, err := f.ReadAt(buf[:readSize], offset); err != nil {
			return nil, err
		}

		// Count newlines backwards in this chunk.
		for i := readSize - 1; i >= 0; i-- {
			if buf[i] == '\n' {
				newlinesFound++
				if n > 0 && newlinesFound > n {
					cutoff = offset + i + 1
					break scan
				}
			}
		}
	}

	data := make([]byte, size-cutoff)
	if _, err := f.ReadAt(data, cutoff); err != nil {
		return nil, err
	}
	return splitTailLines(data, n)
}

// splitTailLines splits byte content on newlines and returns the last n
// lines (or all if n <= 0). It trims leading/trailing whitespace and
// skips empty results.
func splitTailLines(data []byte, n int) ([]string, error) {
	s := strings.TrimSpace(string(data))
	if s == "" {
		return nil, nil
	}
	lines := strings.Split(s, "\n")
	if n > 0 && len(lines) > n {
		lines = lines[len(lines)-n:]
	}
	return lines, nil
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
