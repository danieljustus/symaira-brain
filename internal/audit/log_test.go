package audit

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"
)

func TestRedactArgs_VaultNeverLogsAnything(t *testing.T) {
	tests := []struct {
		server string
		tool   string
		args   string
	}{
		{"vault", "get_entry", `{"id":"secret123","path":"creds/api-key"}`},
		{"vault", "vault_get_entry", `{"password":"hunter2"}`},
		{"vault", "find_entries", `{"query":"api_keys"}`},
		{"vault", "set_entry_field", `{"path":"x","field":"token","value":"super-secret"}`},
		{"vault", "request_credential", `{"path":"x","field":"pw","reason":"need it"}`},
	}

	for _, tt := range tests {
		keys, values := redactArgs(tt.server, tt.tool, json.RawMessage(tt.args), false)
		if keys != "" {
			t.Errorf("vault tool %q: keys = %q, want empty (vault args must never be logged)", tt.tool, keys)
		}
		if values != "" {
			t.Errorf("vault tool %q: values = %q, want empty (vault values must never be logged)", tt.tool, values)
		}
	}
}

func TestRedactArgs_VaultNeverLogsEvenWithVerbose(t *testing.T) {
	args := `{"id":"secret-id","password":"hunter2"}`
	keys, values := redactArgs("vault", "get_entry", json.RawMessage(args), true)
	if keys != "" {
		t.Errorf("vault verbose: keys = %q, want empty", keys)
	}
	if values != "" {
		t.Errorf("vault verbose: values = %q, want empty", values)
	}
}

func TestRedactArgs_NonVaultKeysOnlyByDefault(t *testing.T) {
	args := `{"query":"search term","limit":10}`
	keys, values := redactArgs("memory", "memory_search", json.RawMessage(args), false)

	if keys == "" {
		t.Error("non-vault default: keys should not be empty")
	}
	if values != "" {
		t.Errorf("non-vault default: values = %q, want empty", values)
	}
}

func TestRedactArgs_NonVaultVerboseIncludesValues(t *testing.T) {
	args := `{"query":"search term","limit":10}`
	keys, values := redactArgs("memory", "memory_search", json.RawMessage(args), true)

	if keys == "" {
		t.Error("non-vault verbose: keys should not be empty")
	}
	if values == "" {
		t.Error("non-vault verbose: values should not be empty")
	}
}

func TestRedactArgs_EmptyArgs(t *testing.T) {
	keys, values := redactArgs("vault", "health", nil, false)
	if keys != "" || values != "" {
		t.Errorf("nil args: keys=%q values=%q, want both empty", keys, values)
	}

	keys, values = redactArgs("memory", "memory_search", json.RawMessage(`{}`), false)
	if keys != "" || values != "" {
		t.Errorf("empty object: keys=%q values=%q, want both empty", keys, values)
	}
}

func TestRedactArgs_InvalidJSON(t *testing.T) {
	keys, values := redactArgs("memory", "memory_search", json.RawMessage(`not json`), false)
	if keys != "" || values != "" {
		t.Errorf("invalid JSON: keys=%q values=%q, want both empty", keys, values)
	}
}

func TestRedactArgs_MemoryEntityGraphToolsNotVault(t *testing.T) {
	tests := []struct {
		server string
		tool   string
	}{
		{"memory", "entity_list"},
		{"memory", "graph_neighbors"},
		{"memory", "entity_relate"},
	}

	for _, tt := range tests {
		args := `{"name":"Alice"}`
		keys, _ := redactArgs(tt.server, tt.tool, json.RawMessage(args), false)
		if keys == "" {
			t.Errorf("%s/%s: keys should not be empty (not a vault tool)", tt.server, tt.tool)
		}
	}
}

func TestEntry_JSON(t *testing.T) {
	entry := Entry{
		Timestamp:  "2026-01-01T00:00:00Z",
		Profile:    "personal",
		Server:     "vault",
		Tool:       "get_entry",
		DurationMS: 42,
		Status:     "ok",
	}

	data, err := json.Marshal(entry)
	if err != nil {
		t.Fatalf("Marshal: %v", err)
	}

	var decoded Entry
	if err := json.Unmarshal(data, &decoded); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}

	if decoded.Timestamp != entry.Timestamp {
		t.Errorf("Timestamp = %q, want %q", decoded.Timestamp, entry.Timestamp)
	}
	if decoded.Server != entry.Server {
		t.Errorf("Server = %q, want %q", decoded.Server, entry.Server)
	}
	if decoded.Tool != entry.Tool {
		t.Errorf("Tool = %q, want %q", decoded.Tool, entry.Tool)
	}
}

func TestRedactArgs_KnownVaultPrefixes(t *testing.T) {
	prefixes := []string{"vault_", "get_entry", "find_entries", "set_entry_field", "symaira_"}
	for _, prefix := range prefixes {
		args := `{"key":"value"}`
		keys, values := redactArgs("vault", prefix+"test", json.RawMessage(args), true)
		if keys != "" || values != "" {
			t.Errorf("vault prefix %q: keys=%q values=%q, want both empty", prefix, keys, values)
		}
	}
}

func TestOpen_DisabledReturnsNoOpLogger(t *testing.T) {
	l, err := Open("test", Config{Enabled: false})
	if err != nil {
		t.Fatalf("Open disabled: unexpected error: %v", err)
	}
	if l.f != nil {
		t.Error("disabled logger should have nil file handle")
	}
	if l.config.Enabled {
		t.Error("disabled logger should have Enabled=false")
	}
}

func TestOpen_CreatesDirAndFile(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("XDG_DATA_HOME", dir)

	l, err := Open("test-profile", Config{Enabled: true})
	if err != nil {
		t.Fatalf("Open: %v", err)
	}
	defer l.Close()

	auditDir := filepath.Join(dir, "symbrain", "audit")
	info, err := os.Stat(auditDir)
	if err != nil {
		t.Fatalf("audit dir should exist: %v", err)
	}
	if !info.IsDir() {
		t.Error("audit path should be a directory")
	}

	logPath := filepath.Join(auditDir, "test-profile.jsonl")
	if _, err := os.Stat(logPath); err != nil {
		t.Errorf("log file should exist: %v", err)
	}

	if l.path != logPath {
		t.Errorf("l.path = %q, want %q", l.path, logPath)
	}
	if l.profile != "test-profile" {
		t.Errorf("l.profile = %q, want %q", l.profile, "test-profile")
	}
}

func TestOpen_ExistingFilePreservesSize(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("XDG_DATA_HOME", dir)

	auditDir := filepath.Join(dir, "symbrain", "audit")
	if err := os.MkdirAll(auditDir, 0o700); err != nil {
		t.Fatal(err)
	}
	logPath := filepath.Join(auditDir, "prof.jsonl")
	existing := []byte(`{"timestamp":"2026-01-01T00:00:00Z","profile":"prof","server":"vault","tool":"x","status":"ok"}` + "\n")
	if err := os.WriteFile(logPath, existing, 0o600); err != nil {
		t.Fatal(err)
	}

	l, err := Open("prof", Config{Enabled: true})
	if err != nil {
		t.Fatalf("Open: %v", err)
	}
	defer l.Close()

	if l.size != int64(len(existing)) {
		t.Errorf("l.size = %d, want %d", l.size, len(existing))
	}
}

func newTestFile(t *testing.T) *os.File {
	t.Helper()
	f, err := os.CreateTemp(t.TempDir(), "audit-test-*")
	if err != nil {
		t.Fatal(err)
	}
	return f
}

func TestLog_WritesJSONL(t *testing.T) {
	f := newTestFile(t)
	l := &Logger{
		f:       f,
		path:    f.Name(),
		profile: "test",
		config:  Config{Enabled: true},
	}

	args := json.RawMessage(`{"query":"hello","limit":5}`)
	l.Log("memory", "memory_search", args, 42*time.Millisecond, "ok")

	data, err := os.ReadFile(f.Name())
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}

	lines := strings.Split(strings.TrimSpace(string(data)), "\n")
	if len(lines) != 1 {
		t.Fatalf("expected 1 line, got %d", len(lines))
	}

	var entry Entry
	if err := json.Unmarshal([]byte(lines[0]), &entry); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}
	if entry.Server != "memory" {
		t.Errorf("Server = %q, want %q", entry.Server, "memory")
	}
	if entry.Tool != "memory_search" {
		t.Errorf("Tool = %q, want %q", entry.Tool, "memory_search")
	}
	if entry.DurationMS != 42 {
		t.Errorf("DurationMS = %d, want %d", entry.DurationMS, 42)
	}
	if entry.Status != "ok" {
		t.Errorf("Status = %q, want %q", entry.Status, "ok")
	}
	if entry.Profile != "test" {
		t.Errorf("Profile = %q, want %q", entry.Profile, "test")
	}
	if entry.ArgKeys == "" {
		t.Error("ArgKeys should not be empty for non-vault tool")
	}
	if entry.Timestamp == "" {
		t.Error("Timestamp should not be empty")
	}
}

func TestLog_VaultArgsRedacted(t *testing.T) {
	f := newTestFile(t)
	l := &Logger{
		f:       f,
		path:    f.Name(),
		profile: "test",
		config:  Config{Enabled: true},
	}

	args := json.RawMessage(`{"password":"secret","path":"creds/api-key"}`)
	l.Log("vault", "get_entry", args, 10*time.Millisecond, "ok")

	data, err := os.ReadFile(f.Name())
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}

	var entry Entry
	if err := json.Unmarshal(bytes.TrimSpace(data), &entry); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}
	if entry.ArgKeys != "" {
		t.Errorf("vault ArgKeys = %q, want empty", entry.ArgKeys)
	}
	if entry.ArgValues != "" {
		t.Errorf("vault ArgValues = %q, want empty", entry.ArgValues)
	}
}

func TestLog_NilLoggerNoOp(t *testing.T) {
	var l *Logger
	l.Log("vault", "get_entry", nil, 1*time.Millisecond, "ok")
}

func TestLog_NilFileNoOp(t *testing.T) {
	l := &Logger{config: Config{Enabled: true}}
	l.Log("vault", "get_entry", nil, 1*time.Millisecond, "ok")
}

func TestLog_DisabledNoOp(t *testing.T) {
	f := newTestFile(t)
	l := &Logger{
		f:      f,
		path:   f.Name(),
		config: Config{Enabled: false},
	}
	l.Log("vault", "get_entry", nil, 1*time.Millisecond, "ok")

	data, err := os.ReadFile(f.Name())
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}
	if len(data) != 0 {
		t.Errorf("disabled logger should not write; got %d bytes", len(data))
	}
}

func TestLog_VerboseIncludesValues(t *testing.T) {
	f := newTestFile(t)
	l := &Logger{
		f:       f,
		path:    f.Name(),
		profile: "p",
		config:  Config{Enabled: true, Verbose: true},
	}

	args := json.RawMessage(`{"query":"term","limit":10}`)
	l.Log("memory", "memory_search", args, 5*time.Millisecond, "ok")

	data, err := os.ReadFile(f.Name())
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}

	var entry Entry
	if err := json.Unmarshal(bytes.TrimSpace(data), &entry); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}
	if entry.ArgValues == "" {
		t.Error("verbose mode should include ArgValues")
	}
}

func TestLog_IncrementsSize(t *testing.T) {
	f := newTestFile(t)
	l := &Logger{
		f:       f,
		path:    f.Name(),
		profile: "p",
		config:  Config{Enabled: true},
	}

	if l.size != 0 {
		t.Fatalf("initial size = %d, want 0", l.size)
	}

	l.Log("memory", "tool1", nil, 1*time.Millisecond, "ok")
	if l.size == 0 {
		t.Error("size should increase after Log")
	}
}

func TestClose_NilLogger(t *testing.T) {
	var l *Logger
	if err := l.Close(); err != nil {
		t.Errorf("Close on nil logger: %v", err)
	}
}

func TestClose_NilFile(t *testing.T) {
	l := &Logger{}
	if err := l.Close(); err != nil {
		t.Errorf("Close on nil file: %v", err)
	}
}

func TestClose_ClosesFile(t *testing.T) {
	f := newTestFile(t)
	l := &Logger{
		f:      f,
		config: Config{Enabled: true},
		path:   f.Name(),
	}

	if err := l.Close(); err != nil {
		t.Fatalf("Close: %v", err)
	}

	_, err := f.Write([]byte("test"))
	if err == nil {
		t.Error("write to closed file should fail")
	}
}

func TestClose_DoubleCloseReturnsError(t *testing.T) {
	f := newTestFile(t)
	l := &Logger{
		f:      f,
		config: Config{Enabled: true},
		path:   f.Name(),
	}

	if err := l.Close(); err != nil {
		t.Fatalf("first Close: %v", err)
	}

	err := l.Close()
	if err == nil {
		t.Error("double Close should return error")
	}
}

func TestTail_SpecificProfile(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("XDG_DATA_HOME", dir)

	auditDir := filepath.Join(dir, "symbrain", "audit")
	if err := os.MkdirAll(auditDir, 0o700); err != nil {
		t.Fatal(err)
	}

	entries := []Entry{
		{Timestamp: "2026-01-01T00:00:00Z", Profile: "personal", Server: "vault", Tool: "get_entry", DurationMS: 10, Status: "ok"},
		{Timestamp: "2026-01-01T00:01:00Z", Profile: "personal", Server: "memory", Tool: "search", DurationMS: 20, Status: "ok"},
		{Timestamp: "2026-01-01T00:02:00Z", Profile: "personal", Server: "vault", Tool: "set_entry", DurationMS: 30, Status: "error"},
	}

	var buf bytes.Buffer
	for _, e := range entries {
		data, _ := json.Marshal(e)
		buf.Write(data)
		buf.WriteByte('\n')
	}
	if err := os.WriteFile(filepath.Join(auditDir, "personal.jsonl"), buf.Bytes(), 0o600); err != nil {
		t.Fatal(err)
	}

	var out bytes.Buffer
	if err := Tail(&out, "personal", 2); err != nil {
		t.Fatalf("Tail: %v", err)
	}

	output := out.String()
	lines := strings.Split(strings.TrimSpace(output), "\n")
	if len(lines) != 2 {
		t.Fatalf("expected 2 lines, got %d: %q", len(lines), output)
	}

	if !strings.Contains(lines[0], "search") {
		t.Errorf("first line should contain 'search': %q", lines[0])
	}
	if !strings.Contains(lines[1], "set_entry") {
		t.Errorf("second line should contain 'set_entry': %q", lines[1])
	}
}

func TestTail_AllProfiles(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("XDG_DATA_HOME", dir)

	auditDir := filepath.Join(dir, "symbrain", "audit")
	if err := os.MkdirAll(auditDir, 0o700); err != nil {
		t.Fatal(err)
	}

	for _, prof := range []string{"alpha", "beta"} {
		entry := Entry{
			Timestamp: "2026-01-01T00:00:00Z",
			Profile:   prof,
			Server:    "vault",
			Tool:      "health",
			Status:    "ok",
		}
		data, _ := json.Marshal(entry)
		data = append(data, '\n')
		os.WriteFile(filepath.Join(auditDir, prof+".jsonl"), data, 0o600)
	}

	var out bytes.Buffer
	if err := Tail(&out, "", 0); err != nil {
		t.Fatalf("Tail: %v", err)
	}

	output := out.String()
	if !strings.Contains(output, "alpha") {
		t.Error("output should contain alpha profile")
	}
	if !strings.Contains(output, "beta") {
		t.Error("output should contain beta profile")
	}
}

func TestTail_NoEntries(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("XDG_DATA_HOME", dir)

	auditDir := filepath.Join(dir, "symbrain", "audit")
	if err := os.MkdirAll(auditDir, 0o700); err != nil {
		t.Fatal(err)
	}

	var out bytes.Buffer
	if err := Tail(&out, "nonexistent", 10); err != nil {
		t.Fatalf("Tail: %v", err)
	}
	if out.Len() != 0 {
		t.Errorf("expected empty output, got %d bytes", out.Len())
	}
}

func TestTail_SkipsInvalidJSON(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("XDG_DATA_HOME", dir)

	auditDir := filepath.Join(dir, "symbrain", "audit")
	if err := os.MkdirAll(auditDir, 0o700); err != nil {
		t.Fatal(err)
	}

	content := "not json\n" + `{"timestamp":"2026-01-01T00:00:00Z","profile":"p","server":"vault","tool":"health","status":"ok"}` + "\n"
	if err := os.WriteFile(filepath.Join(auditDir, "p.jsonl"), []byte(content), 0o600); err != nil {
		t.Fatal(err)
	}

	var out bytes.Buffer
	if err := Tail(&out, "p", 0); err != nil {
		t.Fatalf("Tail: %v", err)
	}

	lines := strings.Split(strings.TrimSpace(out.String()), "\n")
	if len(lines) != 1 {
		t.Fatalf("expected 1 valid line, got %d", len(lines))
	}
}

func TestRotate_CreatesBackup(t *testing.T) {
	dir := t.TempDir()
	logPath := filepath.Join(dir, "test.jsonl")

	f, err := os.Create(logPath)
	if err != nil {
		t.Fatal(err)
	}
	f.Write([]byte(`{"data":"existing"}` + "\n"))
	f.Close()

	f, err = os.OpenFile(logPath, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o600)
	if err != nil {
		t.Fatal(err)
	}

	l := &Logger{
		f:    f,
		path: logPath,
		size: maxFileSize,
	}

	l.rotate()

	backupPath := logPath + ".1"
	if _, err := os.Stat(backupPath); err != nil {
		t.Errorf("backup file should exist: %v", err)
	}

	if _, err := os.Stat(logPath); err != nil {
		t.Errorf("new log file should exist: %v", err)
	}

	if l.size != 0 {
		t.Errorf("size should be 0 after rotation, got %d", l.size)
	}

	l.f.Close()
}

func TestRotate_ShiftsExistingBackups(t *testing.T) {
	dir := t.TempDir()
	logPath := filepath.Join(dir, "test.jsonl")

	os.WriteFile(logPath+".1", []byte("backup1\n"), 0o600)
	os.WriteFile(logPath+".2", []byte("backup2\n"), 0o600)
	os.WriteFile(logPath, []byte("current\n"), 0o600)

	f, err := os.OpenFile(logPath, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o600)
	if err != nil {
		t.Fatal(err)
	}

	l := &Logger{
		f:    f,
		path: logPath,
		size: maxFileSize,
	}

	l.rotate()

	for _, suf := range []string{".1", ".2"} {
		if _, err := os.Stat(logPath + suf); err != nil {
			t.Errorf("backup %s should exist: %v", suf, err)
		}
	}

	data, err := os.ReadFile(logPath + ".3")
	if err != nil {
		t.Fatalf("backup .3 should exist: %v", err)
	}
	if string(data) != "backup2\n" {
		t.Errorf(".3 content = %q, want %q", string(data), "backup2\n")
	}

	l.f.Close()
}

func TestRotate_RemovesOldestBeyondMax(t *testing.T) {
	dir := t.TempDir()
	logPath := filepath.Join(dir, "test.jsonl")

	for i := 1; i <= maxBackups; i++ {
		os.WriteFile(logPath+"."+strconv.Itoa(i), []byte("old\n"), 0o600)
	}
	os.WriteFile(logPath, []byte("current\n"), 0o600)

	f, err := os.OpenFile(logPath, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o600)
	if err != nil {
		t.Fatal(err)
	}

	l := &Logger{
		f:    f,
		path: logPath,
		size: maxFileSize,
	}

	l.rotate()

	if _, err := os.Stat(logPath + ".6"); err != nil {
		t.Errorf("backup .6 should exist (old .5 shifted here): %v", err)
	}

	if _, err := os.Stat(logPath + ".5"); err != nil {
		t.Errorf("backup .5 should exist: %v", err)
	}

	l.f.Close()
}

func TestPrintEntry_FormatsOutput(t *testing.T) {
	e := Entry{
		Timestamp:  "2026-06-15T14:30:00Z",
		Profile:    "personal",
		Server:     "vault",
		Tool:       "get_entry",
		DurationMS: 42,
		Status:     "ok",
	}

	var buf bytes.Buffer
	printEntry(&buf, e)

	output := buf.String()

	if !strings.Contains(output, "personal") {
		t.Errorf("output should contain profile: %q", output)
	}
	if !strings.Contains(output, "vault") {
		t.Errorf("output should contain server: %q", output)
	}
	if !strings.Contains(output, "get_entry") {
		t.Errorf("output should contain tool: %q", output)
	}
	if !strings.Contains(output, "42ms") {
		t.Errorf("output should contain duration: %q", output)
	}
	if !strings.Contains(output, "ok") {
		t.Errorf("output should contain status: %q", output)
	}
	if !strings.Contains(output, "2026-06-15") {
		t.Errorf("output should contain date: %q", output)
	}
}

func TestPrintEntry_WithArgKeys(t *testing.T) {
	e := Entry{
		Timestamp: "2026-06-15T14:30:00Z",
		Profile:   "p",
		Server:    "memory",
		Tool:      "search",
		Status:    "ok",
		ArgKeys:   "query,limit",
	}

	var buf bytes.Buffer
	printEntry(&buf, e)

	output := buf.String()
	if !strings.Contains(output, "keys=query,limit") {
		t.Errorf("output should contain keys: %q", output)
	}
}

func TestPrintEntry_NoArgKeysOmitsKeysField(t *testing.T) {
	e := Entry{
		Timestamp: "2026-06-15T14:30:00Z",
		Profile:   "p",
		Server:    "vault",
		Tool:      "health",
		Status:    "ok",
	}

	var buf bytes.Buffer
	printEntry(&buf, e)

	output := buf.String()
	if strings.Contains(output, "keys=") {
		t.Errorf("output should not contain keys= when ArgKeys is empty: %q", output)
	}
}
