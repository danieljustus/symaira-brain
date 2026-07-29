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
)

// Entry is one JSONL audit record for a routed tool call.
type Entry struct {
	Timestamp  string `json:"timestamp"`
	Profile    string `json:"profile"`
	Server     string `json:"server"`
	Tool       string `json:"tool"`
	DurationMS int64  `json:"duration_ms"`
	Status     string `json:"status"`
	ArgKeys    string `json:"arg_keys,omitempty"`
	ArgValues  string `json:"arg_values,omitempty"`
}

// Config controls audit logging behavior.
type Config struct {
	Enabled bool
	Verbose bool
}

// Logger writes JSONL audit entries with strict redaction. It is safe
// for concurrent use.
type Logger struct {
	mu       sync.Mutex
	f        *os.File
	path     string
	profile  string
	config   Config
	size     int64
	degraded bool  // true after any write or rotation failure
	dropped  int64 // total entries dropped due to write failures
}

// maxFileSize is the size threshold for log rotation (10 MB).
const maxFileSize = 10 * 1024 * 1024

// maxBackups is the number of rotated log files to keep.
const maxBackups = 5

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
	f, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o600)
	if err != nil {
		return nil, fmt.Errorf("audit: open %s: %w", path, err)
	}

	info, _ := f.Stat()
	var size int64
	if info != nil {
		size = info.Size()
	}

	return &Logger{
		f:       f,
		path:    path,
		profile: profile,
		config:  cfg,
		size:    size,
	}, nil
}

// Log records a tool call. server is "vault", "memory", or "skills".
// args are the raw JSON arguments (may be nil). duration is the call
// wall-clock time. status is "ok", "error", or "timeout".
func (l *Logger) Log(server, tool string, args json.RawMessage, duration time.Duration, status string) {
	if l == nil || l.f == nil || !l.config.Enabled {
		return
	}

	entry := Entry{
		Timestamp:  time.Now().UTC().Format(time.RFC3339Nano),
		Profile:    l.profile,
		Server:     server,
		Tool:       tool,
		DurationMS: duration.Milliseconds(),
		Status:     status,
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
	data = append(data, '\n')

	n, err := l.f.Write(data)
	if err != nil {
		l.degraded = true
		l.dropped++
	}
	if n > 0 {
		l.size += int64(n)
	}

	if l.size >= maxFileSize {
		l.rotate()
	}
}

// redactArgs applies the redaction policy:
//   - vault_* tools: never log arguments or values in any mode
//   - other servers: log argument KEYS only by default;
//     verbose=true logs values too (still never for vault)
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

	keyList := make([]string, 0, len(m))
	for k := range m {
		keyList = append(keyList, k)
	}
	keys = strings.Join(keyList, ",")

	if verbose {
		valParts := make([]string, 0, len(m))
		for _, k := range keyList {
			valParts = append(valParts, fmt.Sprintf("%s=%v", k, m[k]))
		}
		values = strings.Join(valParts, ",")
	}

	return keys, values
}

func (l *Logger) rotate() {
	if l.f != nil {
		l.f.Close()
	}

	for i := maxBackups; i >= 1; i-- {
		old := fmt.Sprintf("%s.%d", l.path, i)
		newPath := fmt.Sprintf("%s.%d", l.path, i+1)
		if i+1 > maxBackups {
			os.Remove(newPath)
		}
		os.Rename(old, newPath)
	}
	os.Rename(l.path, l.path+".1")

	f, err := os.OpenFile(l.path, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o600)
	if err != nil {
		l.degraded = true
		l.f = nil
		return
	}
	l.f = f
	l.size = 0
}

// Close closes the underlying file. The logger is unusable afterward.
func (l *Logger) Close() error {
	if l == nil || l.f == nil {
		return nil
	}
	l.mu.Lock()
	defer l.mu.Unlock()
	return l.f.Close()
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
