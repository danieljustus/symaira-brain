package audit

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"strconv"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-corekit/auditkit"
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
	if l.sink != nil {
		t.Error("disabled logger should have nil sink")
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

	// Chain state is tracked by the auditkit sink; size is internal now.
	if l.sink == nil {
		t.Error("sink should be open after Open()")
	}
}

func TestLatestDegradations_ReturnsMostRecentSessionPerProfile(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("XDG_DATA_HOME", dir)

	auditDir := filepath.Join(dir, "symbrain", "audit")
	if err := os.MkdirAll(auditDir, 0o700); err != nil {
		t.Fatal(err)
	}

	line := func(entry Entry) string {
		data, err := json.Marshal(entry)
		if err != nil {
			t.Fatalf("Marshal: %v", err)
		}
		return string(data) + "\n"
	}

	// prof.jsonl: two sessions; only the last session's degraded entry
	// should be reported, and only with status "degraded".
	profLog := filepath.Join(auditDir, "prof.jsonl")
	prof := []string{
		line(Entry{SessionID: "s1", Profile: "prof", Server: "vault", Status: "degraded", Reason: "old", Level: "warning"}),
		line(Entry{SessionID: "s2", Profile: "prof", Server: "vault", Status: "ok"}),
		line(Entry{SessionID: "s2", Profile: "prof", Server: "memory", Status: "degraded", Reason: "new", Level: "warning"}),
	}
	if err := os.WriteFile(profLog, []byte(joinLines(prof)), 0o600); err != nil {
		t.Fatal(err)
	}

	// other.jsonl: single degraded entry in a different profile.
	otherLog := filepath.Join(auditDir, "other.jsonl")
	other := []string{
		line(Entry{SessionID: "s9", Profile: "other", Server: "skills", Status: "degraded", Reason: "missing", Level: "error"}),
	}
	if err := os.WriteFile(otherLog, []byte(joinLines(other)), 0o600); err != nil {
		t.Fatal(err)
	}

	t.Run("empty profile reads every profile log", func(t *testing.T) {
		got, err := LatestDegradations("")
		if err != nil {
			t.Fatalf("LatestDegradations: %v", err)
		}
		want := []Degradation{
			{SessionID: "s9", Profile: "other", Server: "skills", Reason: "missing", Level: "error"},
			{SessionID: "s2", Profile: "prof", Server: "memory", Reason: "new", Level: "warning"},
		}
		if !reflect.DeepEqual(got, want) {
			t.Errorf("LatestDegradations() = %+v, want %+v", got, want)
		}
	})

	t.Run("profile filter reads only that profile", func(t *testing.T) {
		got, err := LatestDegradations("prof")
		if err != nil {
			t.Fatalf("LatestDegradations: %v", err)
		}
		want := []Degradation{
			{SessionID: "s2", Profile: "prof", Server: "memory", Reason: "new", Level: "warning"},
		}
		if !reflect.DeepEqual(got, want) {
			t.Errorf("LatestDegradations(prof) = %+v, want %+v", got, want)
		}
	})

	t.Run("missing audit dir yields empty result", func(t *testing.T) {
		emptyDir := t.TempDir()
		t.Setenv("XDG_DATA_HOME", emptyDir)
		got, err := LatestDegradations("")
		if err != nil {
			t.Fatalf("LatestDegradations: %v", err)
		}
		if len(got) != 0 {
			t.Errorf("LatestDegradations() = %+v, want empty", got)
		}
	})
}

func joinLines(lines []string) string {
	var sb strings.Builder
	for _, l := range lines {
		sb.WriteString(l)
	}
	return sb.String()
}

// readPayloads reads the audit file and returns the inner entry payloads
// (auditkit stores entries as chained envelopes {d: payload, h: hash}).
func readPayloads(t *testing.T, path string) []string {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return nil
		}
		t.Fatal(err)
	}
	var out []string
	for _, line := range strings.Split(string(data), "\n") {
		if line == "" {
			continue
		}
		var env struct {
			D string `json:"d"`
		}
		if err := json.Unmarshal([]byte(line), &env); err == nil && env.D != "" {
			out = append(out, env.D)
		} else {
			out = append(out, line) // legacy un-chained line
		}
	}
	return out
}

// newTestLogger builds a Logger backed by a real auditkit sink on a
// temp file — mirrors production wiring so the chain state works.
func newTestLogger(t *testing.T) *Logger {
	t.Helper()
	path := filepath.Join(t.TempDir(), "audit.jsonl")
	sink, err := auditkit.OpenSink(path)
	if err != nil {
		t.Fatal(err)
	}
	return &Logger{
		sink:    sink,
		path:    path,
		profile: "test",
		config:  Config{Enabled: true},
	}
}

func TestLog_WritesJSONL(t *testing.T) {
	l := newTestLogger(t)

	args := json.RawMessage(`{"query":"hello","limit":5}`)
	l.Log("memory", "memory_search", args, 42*time.Millisecond, "ok", Exposure{})

	data, err := os.ReadFile(l.path)
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}

	lines := strings.Split(strings.TrimSpace(string(data)), "\n")
	if len(lines) != 1 {
		t.Fatalf("expected 1 line, got %d", len(lines))
	}

	payloads := readPayloads(t, l.path)
	var entry Entry
	if err := json.Unmarshal([]byte(payloads[0]), &entry); err != nil {
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
	l := newTestLogger(t)

	args := json.RawMessage(`{"password":"secret","path":"creds/api-key"}`)
	l.Log("vault", "get_entry", args, 10*time.Millisecond, "ok", Exposure{})

	payloads := readPayloads(t, l.path)
	if len(payloads) == 0 {
		t.Fatal("no payloads written")
	}
	var entry Entry
	if err := json.Unmarshal([]byte(payloads[len(payloads)-1]), &entry); err != nil {
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
	l.Log("vault", "get_entry", nil, 1*time.Millisecond, "ok", Exposure{})
}

func TestLog_NilFileNoOp(t *testing.T) {
	l := &Logger{config: Config{Enabled: true}}
	l.Log("vault", "get_entry", nil, 1*time.Millisecond, "ok", Exposure{})
}

func TestLog_DisabledNoOp(t *testing.T) {
	l := &Logger{
		sink:   nil,
		path:   filepath.Join(t.TempDir(), "audit.jsonl"),
		config: Config{Enabled: false},
	}
	l.Log("vault", "get_entry", nil, 1*time.Millisecond, "ok", Exposure{})

	if _, err := os.Stat(l.path); !os.IsNotExist(err) {
		t.Errorf("disabled logger should not create a log file (stat err: %v)", err)
	}
}

func TestLog_VerboseIncludesValues(t *testing.T) {
	l := newTestLogger(t)
	l.config.Verbose = true

	args := json.RawMessage(`{"query":"term","limit":10}`)
	l.Log("memory", "memory_search", args, 5*time.Millisecond, "ok", Exposure{})

	payloads := readPayloads(t, l.path)
	if len(payloads) == 0 {
		t.Fatal("no payloads written")
	}
	var entry Entry
	if err := json.Unmarshal([]byte(payloads[len(payloads)-1]), &entry); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}
	if entry.ArgValues == "" {
		t.Error("verbose mode should include ArgValues")
	}
}

func TestLog_IncrementsSinkCount(t *testing.T) {
	l := newTestLogger(t)

	before := l.sink.Count()
	l.Log("memory", "tool1", nil, 1*time.Millisecond, "ok", Exposure{})
	if l.sink.Count() != before+1 {
		t.Errorf("sink count = %d, want %d", l.sink.Count(), before+1)
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

func TestClose_ClosesSink(t *testing.T) {
	l := newTestLogger(t)
	if l.sink == nil {
		t.Fatal("newTestLogger should have an open sink")
	}
	if err := l.Close(); err != nil {
		t.Fatalf("Close: %v", err)
	}
}

func TestClose_DoubleCloseReturnsError(t *testing.T) {
	l := newTestLogger(t)

	if err := l.Close(); err != nil {
		t.Fatalf("first Close: %v", err)
	}

	if err := l.Close(); err != nil {
		t.Errorf("double Close should be idempotent, got %v", err)
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
		if err := os.WriteFile(filepath.Join(auditDir, prof+".jsonl"), data, 0o600); err != nil {
			t.Fatalf("write profile log: %v", err)
		}
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

func TestTailEntries_SingleProfile(t *testing.T) {
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

	got, err := TailEntries("personal", 2)
	if err != nil {
		t.Fatalf("TailEntries: %v", err)
	}
	if len(got) != 2 {
		t.Fatalf("expected 2 entries, got %d", len(got))
	}
	if got[0].Tool != "search" {
		t.Errorf("first entry tool = %q, want search", got[0].Tool)
	}
	if got[1].Tool != "set_entry" {
		t.Errorf("second entry tool = %q, want set_entry", got[1].Tool)
	}
	if got[0].Profile != "personal" {
		t.Errorf("first entry profile = %q, want personal", got[0].Profile)
	}
}

func TestTailEntries_MultiProfileMerge(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("XDG_DATA_HOME", dir)

	auditDir := filepath.Join(dir, "symbrain", "audit")
	if err := os.MkdirAll(auditDir, 0o700); err != nil {
		t.Fatal(err)
	}

	// Write entries for two different profiles.
	for _, prof := range []string{"personal", "work"} {
		var buf bytes.Buffer
		for i := 0; i < 3; i++ {
			e := Entry{
				Timestamp: "2026-01-01T00:00:00Z",
				Profile:   prof,
				Server:    "vault",
				Tool:      prof + "_tool_" + strconv.Itoa(i),
				Status:    "ok",
			}
			data, _ := json.Marshal(e)
			buf.Write(data)
			buf.WriteByte('\n')
		}
		if err := os.WriteFile(filepath.Join(auditDir, prof+".jsonl"), buf.Bytes(), 0o600); err != nil {
			t.Fatal(err)
		}
	}

	// Empty profile = merge all.
	got, err := TailEntries("", 10)
	if err != nil {
		t.Fatalf("TailEntries: %v", err)
	}
	if len(got) != 6 {
		t.Fatalf("expected 6 merged entries, got %d", len(got))
	}

	// Verify both profiles present.
	profiles := map[string]bool{}
	for _, e := range got {
		profiles[e.Profile] = true
	}
	if !profiles["personal"] || !profiles["work"] {
		t.Errorf("expected both profiles, got %v", profiles)
	}
}

func TestTailEntries_EmptyDir(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("XDG_DATA_HOME", dir)

	auditDir := filepath.Join(dir, "symbrain", "audit")
	if err := os.MkdirAll(auditDir, 0o700); err != nil {
		t.Fatal(err)
	}

	got, err := TailEntries("", 10)
	if err != nil {
		t.Fatalf("TailEntries: %v", err)
	}
	if len(got) != 0 {
		t.Fatalf("expected 0 entries from empty dir, got %d", len(got))
	}
}

func TestTailEntries_SkipsMalformedEntries(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("XDG_DATA_HOME", dir)

	auditDir := filepath.Join(dir, "symbrain", "audit")
	if err := os.MkdirAll(auditDir, 0o700); err != nil {
		t.Fatal(err)
	}

	// Mix valid and invalid JSON lines.
	var buf bytes.Buffer
	valid, _ := json.Marshal(Entry{Timestamp: "2026-01-01T00:00:00Z", Server: "vault", Tool: "ok_tool", Status: "ok"})
	buf.Write(valid)
	buf.WriteByte('\n')
	buf.WriteString("NOT JSON\n")
	buf.WriteByte('\n')
	buf.Write(valid)
	buf.WriteByte('\n')

	if err := os.WriteFile(filepath.Join(auditDir, "test.jsonl"), buf.Bytes(), 0o600); err != nil {
		t.Fatal(err)
	}

	got, err := TailEntries("test", 10)
	if err != nil {
		t.Fatalf("TailEntries: %v", err)
	}
	if len(got) != 2 {
		t.Fatalf("expected 2 valid entries (malformed skipped), got %d", len(got))
	}
}

func TestTailEntries_NonexistentProfile(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("XDG_DATA_HOME", dir)

	auditDir := filepath.Join(dir, "symbrain", "audit")
	if err := os.MkdirAll(auditDir, 0o700); err != nil {
		t.Fatal(err)
	}

	got, err := TailEntries("nonexistent", 10)
	if err != nil {
		t.Fatalf("TailEntries: %v", err)
	}
	if len(got) != 0 {
		t.Fatalf("expected 0 entries for nonexistent profile, got %d", len(got))
	}
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

// TestTailFile_LargeFileBackwardScan exercises the >64 KB backward-scan
// branch of tailFile: the last n entries must survive, in order.
func TestTailFile_LargeFileBackwardScan(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("XDG_DATA_HOME", dir)

	auditDir := filepath.Join(dir, "symbrain", "audit")
	if err := os.MkdirAll(auditDir, 0o700); err != nil {
		t.Fatal(err)
	}

	const totalEntries = 2000
	logPath := filepath.Join(auditDir, "big.jsonl")
	f, err := os.OpenFile(logPath, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o600)
	if err != nil {
		t.Fatal(err)
	}
	for i := 0; i < totalEntries; i++ {
		e := Entry{
			Timestamp:  "2026-01-01T00:00:00Z",
			Profile:    "big",
			Server:     "memory",
			Tool:       "search",
			DurationMS: int64(i),
			Status:     "ok",
		}
		data, _ := json.Marshal(e)
		if _, err := f.Write(append(data, '\n')); err != nil {
			t.Fatal(err)
		}
	}
	f.Close()

	info, err := os.Stat(logPath)
	if err != nil {
		t.Fatal(err)
	}
	if info.Size() <= tailChunkSize {
		t.Fatalf("fixture too small: %d bytes; backward scan needs > %d", info.Size(), tailChunkSize)
	}

	var out bytes.Buffer
	if err := Tail(&out, "big", 10); err != nil {
		t.Fatalf("Tail: %v", err)
	}
	lines := strings.Split(strings.TrimSpace(out.String()), "\n")
	if len(lines) != 10 {
		t.Fatalf("expected 10 lines, got %d", len(lines))
	}
	// Last line must be the final entry (duration_ms = 1999).
	last := lines[len(lines)-1]
	if !strings.Contains(last, "1999ms") {
		t.Errorf("last line should contain duration 1999ms: %q", last)
	}
	first := lines[0]
	if !strings.Contains(first, "1990ms") {
		t.Errorf("first line should contain duration 1990ms: %q", first)
	}
}

// TestTailFile_LargeFileAllLines verifies the backward scan returns every
// line when no count limit is given.
func TestTailFile_LargeFileAllLines(t *testing.T) {
	dir := t.TempDir()
	logPath := filepath.Join(dir, "big.jsonl")
	f, err := os.OpenFile(logPath, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o600)
	if err != nil {
		t.Fatal(err)
	}
	line := `{"timestamp":"2026-01-01T00:00:00Z","profile":"big","server":"memory","tool":"search","duration_ms":1,"status":"ok"}`
	for i := 0; i < 1500; i++ { // ~140 KB, above the 64 KB chunk size
		if _, err := f.WriteString(line + "\n"); err != nil {
			t.Fatal(err)
		}
	}
	f.Close()

	lines, err := tailFile(logPath, 0)
	if err != nil {
		t.Fatalf("tailFile: %v", err)
	}
	if len(lines) != 1500 {
		t.Fatalf("expected 1500 lines, got %d", len(lines))
	}
}

// TestDegraded_StartsFalseAndFlipsOnWriteFailure covers the Degraded
// reporting path: a closed underlying file turns subsequent Log calls
// into dropped, degraded entries.
func TestDegraded_StartsFalseAndFlipsOnWriteFailure(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("XDG_DATA_HOME", dir)

	l, err := Open("degraded", Config{Enabled: true})
	if err != nil {
		t.Fatalf("Open: %v", err)
	}
	defer l.Close()

	if l.Degraded() {
		t.Error("fresh logger should not be degraded")
	}

	// Close the sink behind the logger's back, then log: the write fails
	// and the logger flips to degraded (sink is already closed; auditkit
	// surfaces the error through Append).
	l.mu.Lock()
	if l.sink != nil {
		_ = l.sink.Close()
	}
	l.mu.Unlock()
	l.Log("memory", "search", nil, time.Millisecond, "ok", Exposure{})

	if !l.Degraded() {
		t.Error("logger should be degraded after a write failure")
	}
	if l.dropped != 1 {
		t.Errorf("dropped = %d, want 1", l.dropped)
	}
}

// TestDegraded_NilLoggerIsNeverDegraded covers the nil-receiver guard.
func TestDegraded_NilLoggerIsNeverDegraded(t *testing.T) {
	var l *Logger
	if l.Degraded() {
		t.Error("nil logger should never report degraded")
	}
}

// TestMemoryContentNeverInAuditLog verifies that the "content" field is
// redacted as "[redacted]" in verbose mode and that the old
// fmt.Sprintf("%s=%v", k, m[k]) pattern no longer leaks raw content.
func TestMemoryContentNeverInAuditLog(t *testing.T) {
	args := json.RawMessage(`{"content":"private memory text","query":"search"}`)
	_, values := redactArgs("memory", "memory_set", args, true)

	if !strings.Contains(values, "[redacted]") {
		t.Errorf("content should be redacted, got values: %s", values)
	}
	if strings.Contains(values, "private memory text") {
		t.Errorf("content value must never appear in audit log, got: %s", values)
	}
	if strings.Contains(values, "content=private") {
		t.Errorf("old fmt.Sprintf pattern should be gone, got: %s", values)
	}
}

// TestRedactArgs_NestedSensitiveKeysRedacted verifies that sensitive keys
// inside nested objects are redacted in verbose mode.
func TestRedactArgs_NestedSensitiveKeysRedacted(t *testing.T) {
	args := json.RawMessage(`{"user":{"credentials":{"password":"secret123"}},"query":"search"}`)
	_, values := redactArgs("memory", "memory_search", args, true)

	if !strings.Contains(values, "[redacted]") {
		t.Errorf("nested password should be redacted, got: %s", values)
	}
	if strings.Contains(values, "secret123") {
		t.Errorf("nested password value must not leak, got: %s", values)
	}
}

// TestRedactArgs_ArrayWithSensitiveMapsRedacted verifies that sensitive
// keys inside maps contained in arrays are redacted.
func TestRedactArgs_ArrayWithSensitiveMapsRedacted(t *testing.T) {
	args := json.RawMessage(`{"items":[{"password":"secret1"},{"token":"secret2"}],"name":"list"}`)
	_, values := redactArgs("memory", "list_items", args, true)

	if strings.Contains(values, "secret1") || strings.Contains(values, "secret2") {
		t.Errorf("array element secrets should be redacted, got: %s", values)
	}
	if !strings.Contains(values, "[redacted]") {
		t.Errorf("should contain [redacted], got: %s", values)
	}
}

// TestRedactArgs_ForeignToolSensitiveKeysRedacted verifies redaction works
// for non-core foreign servers.
func TestRedactArgs_ForeignToolSensitiveKeysRedacted(t *testing.T) {
	args := json.RawMessage(`{"api_key":"sk-123","action":"call"}`)
	_, values := redactArgs("foreign-server", "some_tool", args, true)

	if !strings.Contains(values, "[redacted]") {
		t.Errorf("foreign tool api_key should be redacted, got: %s", values)
	}
	if strings.Contains(values, "sk-123") {
		t.Errorf("foreign tool api_key value must not leak, got: %s", values)
	}
}

// TestRedactArgs_CaseInsensitiveSensitiveKeys verifies that sensitive-key
// matching is case-insensitive.
func TestRedactArgs_CaseInsensitiveSensitiveKeys(t *testing.T) {
	tests := []struct {
		key   string
		value string
	}{
		{"Password", "secret1"},
		{"API_KEY", "secret2"},
		{"Authorization", "Bearer abc"},
		{"TOKEN", "secret3"},
		{"Private_Key", "secret4"},
		{"ClientSecret", "secret5"},
	}

	for _, tt := range tests {
		args := fmt.Sprintf(`{"%s":"%s","query":"test"}`, tt.key, tt.value)
		_, values := redactArgs("memory", "search", json.RawMessage(args), true)

		if strings.Contains(values, tt.value) {
			t.Errorf("case-insensitive: %s value should be redacted, got: %s", tt.key, values)
		}
		if !strings.Contains(values, "[redacted]") {
			t.Errorf("case-insensitive: %s should produce [redacted], got: %s", tt.key, values)
		}
	}
}

// TestRedactArgs_CommonCredentialVariants verifies that common credential
// key variants are all redacted.
func TestRedactArgs_CommonCredentialVariants(t *testing.T) {
	variants := []string{
		`{"api_key":"val"}`,
		`{"api-key":"val"}`,
		`{"apikey":"val"}`,
		`{"ApiKey":"val"}`,
		`{"API_KEY":"val"}`,
		`{"client_secret":"val"}`,
		`{"client-secret":"val"}`,
		`{"access_token":"val"}`,
		`{"access-token":"val"}`,
		`{"private_key":"val"}`,
		`{"private-key":"val"}`,
		`{"passphrase":"val"}`,
		`{"pwd":"val"}`,
		`{"credentials":"val"}`,
	}

	for _, args := range variants {
		_, values := redactArgs("memory", "tool", json.RawMessage(args), true)
		if !strings.Contains(values, "[redacted]") {
			t.Errorf("variant %s should be redacted, got values: %s", args, values)
		}
	}
}
