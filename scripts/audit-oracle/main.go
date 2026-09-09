package main

import (
	"bytes"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/audit"
	"github.com/danieljustus/symaira-corekit/auditkit"
)

type redactExpectation struct {
	ID        string `json:"id"`
	Server    string `json:"server"`
	Tool      string `json:"tool"`
	Args      string `json:"args"`
	Verbose   bool   `json:"verbose"`
	ArgKeys   string `json:"arg_keys"`
	ArgValues string `json:"arg_values"`
}

type tailExpectation struct {
	Files        map[string]string   `json:"files"`
	Profile      string              `json:"profile"`
	Limit        int                 `json:"limit"`
	Entries      []audit.Entry       `json:"entries"`
	Degradations []audit.Degradation `json:"degradations"`
}

type suite struct {
	EntryJSON          string              `json:"entry_json"`
	Hashes             map[string]string   `json:"hashes"`
	Redactions         []redactExpectation `json:"redactions"`
	Tail               tailExpectation     `json:"tail"`
	ChainedTailEntries []audit.Entry       `json:"chained_tail_entries"`
}

func readPayload(path string) audit.Entry {
	data, err := os.ReadFile(path)
	if err != nil {
		panic(err)
	}
	var envelope struct {
		Data string `json:"d"`
	}
	if err := json.Unmarshal(bytes.TrimSpace(data), &envelope); err != nil {
		panic(err)
	}
	var entry audit.Entry
	if err := json.Unmarshal([]byte(envelope.Data), &entry); err != nil {
		panic(err)
	}
	return entry
}

func loggerRedaction(id, server, tool, args string, verbose bool) redactExpectation {
	dir, err := os.MkdirTemp("", "audit-oracle")
	if err != nil {
		panic(err)
	}
	defer os.RemoveAll(dir)
	old := os.Getenv("XDG_DATA_HOME")
	if err := os.Setenv("XDG_DATA_HOME", dir); err != nil {
		panic(err)
	}
	defer os.Setenv("XDG_DATA_HOME", old)
	logger, err := audit.Open(id, audit.Config{Enabled: true, Verbose: verbose})
	if err != nil {
		panic(err)
	}
	logger.Log(server, tool, json.RawMessage(args), time.Millisecond, "ok", audit.Exposure{})
	if err := logger.Close(); err != nil {
		panic(err)
	}
	entry := readPayload(filepath.Join(dir, "symbrain", "audit", id+".jsonl"))
	return redactExpectation{ID: id, Server: server, Tool: tool, Args: args, Verbose: verbose, ArgKeys: entry.ArgKeys, ArgValues: entry.ArgValues}
}

func entryLine(entry audit.Entry) string {
	data, err := json.Marshal(entry)
	if err != nil {
		panic(err)
	}
	return string(data) + "\n"
}

func tailFixture() tailExpectation {
	files := map[string]string{
		"alpha.jsonl": strings.Join([]string{
			entryLine(audit.Entry{Timestamp: "2026-01-01T00:00:00Z", SessionID: "old", Profile: "ignored", Server: "vault", Tool: "old", Status: "ok"}),
			"NOT JSON\n",
			entryLine(audit.Entry{Timestamp: "2026-01-01T00:01:00Z", SessionID: "new", Profile: "ignored", Server: "memory", Tool: "first", Status: "ok"}),
			entryLine(audit.Entry{Timestamp: "2026-01-01T00:02:00Z", SessionID: "new", Profile: "ignored", Server: "memory", Tool: "second", Status: "degraded", Reason: "missing", Level: "warning"}),
		}, ""),
	}
	dir, err := os.MkdirTemp("", "audit-tail-oracle")
	if err != nil {
		panic(err)
	}
	defer os.RemoveAll(dir)
	auditDir := filepath.Join(dir, "symbrain", "audit")
	if err := os.MkdirAll(auditDir, 0o700); err != nil {
		panic(err)
	}
	for name, content := range files {
		if err := os.WriteFile(filepath.Join(auditDir, name), []byte(content), 0o600); err != nil {
			panic(err)
		}
	}
	old := os.Getenv("XDG_DATA_HOME")
	if err := os.Setenv("XDG_DATA_HOME", dir); err != nil {
		panic(err)
	}
	defer os.Setenv("XDG_DATA_HOME", old)
	entries, err := audit.TailEntries("alpha", 2)
	if err != nil {
		panic(err)
	}
	degradations, err := audit.LatestDegradations("alpha")
	if err != nil {
		panic(err)
	}
	return tailExpectation{Files: files, Profile: "alpha", Limit: 2, Entries: entries, Degradations: degradations}
}

func chainedTailFixture() []audit.Entry {
	dir, err := os.MkdirTemp("", "audit-chain-tail-oracle")
	if err != nil {
		panic(err)
	}
	defer os.RemoveAll(dir)
	old := os.Getenv("XDG_DATA_HOME")
	if err := os.Setenv("XDG_DATA_HOME", dir); err != nil {
		panic(err)
	}
	defer os.Setenv("XDG_DATA_HOME", old)
	logger, err := audit.Open("chain", audit.Config{Enabled: true})
	if err != nil {
		panic(err)
	}
	logger.Log("memory", "memory_search", json.RawMessage(`{"query":"term"}`), time.Millisecond, "ok", audit.Exposure{})
	if err := logger.Close(); err != nil {
		panic(err)
	}
	entries, err := audit.TailEntries("chain", 1)
	if err != nil {
		panic(err)
	}
	return entries
}

func generate() suite {
	entry := audit.Entry{Timestamp: "2026-01-01T00:00:00Z", SessionID: "s1", Profile: "personal", Server: "memory", Tool: "memory_search", DurationMS: 42, Status: "error", Category: "timeout", Retryable: true, ArgKeys: "query", ArgValues: "query=term", AccessClass: "read", AccessSource: "read_only_hint"}
	entryJSON, err := json.Marshal(entry)
	if err != nil {
		panic(err)
	}
	first := auditkit.HashEntry("hello", auditkit.GenesisHash)
	return suite{
		EntryJSON: string(entryJSON),
		Hashes: map[string]string{
			"hello_genesis":     first,
			"world_after_hello": auditkit.HashEntry("world", first),
		},
		Redactions: []redactExpectation{
			loggerRedaction("vault", "vault", "get_entry", `{"password":"secret"}`, true),
			loggerRedaction("keys", "memory", "memory_search", `{"query":"term"}`, false),
			loggerRedaction("verbose", "memory", "memory_set", `{"content":"private"}`, true),
		},
		Tail:               tailFixture(),
		ChainedTailEntries: chainedTailFixture(),
	}
}

func main() {
	check := flag.Bool("check", false, "fail if generated output differs")
	output := flag.String("output", "rust/symbrain-audit/tests/fixtures/oracle_expectations.json", "output path")
	flag.Parse()
	data, err := json.MarshalIndent(generate(), "", "  ")
	if err != nil {
		panic(err)
	}
	data = append(data, '\n')
	if *check {
		existing, err := os.ReadFile(*output)
		if err != nil || !bytes.Equal(existing, data) {
			fmt.Fprintf(os.Stderr, "%s is out of date; run go run ./scripts/audit-oracle\n", *output)
			os.Exit(1)
		}
		fmt.Println("PASS: audit oracle deterministic check passed")
		return
	}
	if err := os.MkdirAll(filepath.Dir(*output), 0o755); err != nil {
		panic(err)
	}
	if err := os.WriteFile(*output, data, 0o644); err != nil {
		panic(err)
	}
	fmt.Printf("Wrote %s\n", *output)
}
