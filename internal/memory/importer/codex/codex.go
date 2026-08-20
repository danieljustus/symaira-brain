package codex

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
)

// CodexImporter imports sessions from Codex CLI.
type CodexImporter struct {
	customPath string
}

// CodexEntry represents a single line in a Codex rollout JSONL file.
type CodexEntry struct {
	Timestamp string       `json:"timestamp"`
	Type      string       `json:"type"`
	Payload   CodexPayload `json:"payload"`
}

// CodexPayload is the message payload inside a Codex entry.
type CodexPayload struct {
	Role    string `json:"role"`
	Content string `json:"content"`
}

func NewCodexImporter(customPath string) *CodexImporter {
	return &CodexImporter{customPath: customPath}
}

func (c *CodexImporter) Name() string {
	return "codex"
}

func (c *CodexImporter) IsTranscript() bool { return true }

func (c *CodexImporter) DiscoverSessions(since time.Time) ([]importer.SessionRef, error) {
	basePath := c.customPath
	if basePath == "" {
		home, err := os.UserHomeDir()
		if err != nil {
			return nil, err
		}
		basePath = filepath.Join(home, ".codex")
	}

	archivedDir := filepath.Join(basePath, "archived_sessions")

	var sessions []importer.SessionRef

	// Discover rollout-*.jsonl files in archived_sessions/.
	err := filepath.Walk(archivedDir, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return nil
		}
		if info.IsDir() {
			return nil
		}
		base := filepath.Base(path)
		if !strings.HasSuffix(path, ".jsonl") {
			return nil
		}
		// Skip the index file; only import actual rollout files.
		if base == "session_index.jsonl" {
			return nil
		}
		if !strings.HasPrefix(base, "rollout-") {
			return nil
		}
		if info.ModTime().Before(since) {
			return nil
		}

		sessions = append(sessions, importer.SessionRef{
			Tool:       "codex",
			SessionID:  strings.TrimSuffix(base, ".jsonl"),
			Path:       path,
			ModifiedAt: info.ModTime(),
			Metadata:   map[string]string{},
		})
		return nil
	})

	return sessions, err
}

func (c *CodexImporter) ImportSession(ref importer.SessionRef) ([]importer.ImportedFact, error) {
	file, err := os.Open(ref.Path)
	if err != nil {
		return nil, fmt.Errorf("failed to read session file: %w", err)
	}
	defer func() { _ = file.Close() }()

	var facts []importer.ImportedFact

	scanner := bufio.NewScanner(file)
	scanner.Buffer(make([]byte, 0, 64*1024), 1024*1024)

	for scanner.Scan() {
		line := scanner.Text()
		if strings.TrimSpace(line) == "" {
			continue
		}

		var entry CodexEntry
		if err := json.Unmarshal([]byte(line), &entry); err != nil {
			continue // skip malformed lines
		}

		// We only care about assistant messages longer than 50 chars.
		if entry.Payload.Role == "assistant" && len(entry.Payload.Content) > 50 {
			ts := parseTimestamp(entry.Timestamp)

			facts = append(facts, importer.ImportedFact{
				Content:   entry.Payload.Content,
				Source:    "codex",
				SessionID: ref.SessionID,
				Timestamp: ts,
				Metadata:  map[string]string{},
			})
		}
	}

	return facts, scanner.Err()
}

// parseTimestamp attempts to parse a timestamp string in common formats.
func parseTimestamp(s string) time.Time {
	if s == "" {
		return time.Time{}
	}
	for _, layout := range []string{
		time.RFC3339Nano,
		time.RFC3339,
		"2006-01-02T15:04:05Z07:00",
	} {
		if t, err := time.Parse(layout, s); err == nil {
			return t
		}
	}
	return time.Time{}
}
