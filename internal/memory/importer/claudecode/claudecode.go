package claudecode

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

// ClaudeCodeImporter imports sessions from Claude Code.
type ClaudeCodeImporter struct {
	customPath string
}

// ClaudeCodeEntry represents a single line in a Claude Code JSONL transcript.
type ClaudeCodeEntry struct {
	UUID      string            `json:"uuid"`
	Type      string            `json:"type"`
	Message   ClaudeCodeMessage `json:"message"`
	Timestamp string            `json:"timestamp"`
	Project   string            `json:"project"`
	Model     string            `json:"model"`
	Metadata  map[string]string `json:"metadata"`
}

// ClaudeCodeMessage represents the message payload within an entry.
type ClaudeCodeMessage struct {
	Role    string `json:"role"`
	Content string `json:"content"`
}

func NewClaudeCodeImporter(customPath string) *ClaudeCodeImporter {
	return &ClaudeCodeImporter{customPath: customPath}
}

func (c *ClaudeCodeImporter) Name() string {
	return "claude-code"
}

func (c *ClaudeCodeImporter) IsTranscript() bool { return true }

func (c *ClaudeCodeImporter) DiscoverSessions(since time.Time) ([]importer.SessionRef, error) {
	basePath := c.customPath
	if basePath == "" {
		home, err := os.UserHomeDir()
		if err != nil {
			return nil, err
		}
		basePath = filepath.Join(home, ".claude", "projects")
	}

	var sessions []importer.SessionRef

	err := filepath.Walk(basePath, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return nil
		}
		if info.IsDir() || !strings.HasSuffix(path, ".jsonl") {
			return nil
		}
		if info.ModTime().Before(since) {
			return nil
		}

		sessions = append(sessions, importer.SessionRef{
			Tool:       "claude-code",
			SessionID:  strings.TrimSuffix(filepath.Base(path), ".jsonl"),
			Path:       path,
			ModifiedAt: info.ModTime(),
			Metadata: map[string]string{
				"project_dir": filepath.Base(filepath.Dir(path)),
			},
		})
		return nil
	})

	return sessions, err
}

func (c *ClaudeCodeImporter) ImportSession(ref importer.SessionRef) ([]importer.ImportedFact, error) {
	file, err := os.Open(ref.Path)
	if err != nil {
		return nil, fmt.Errorf("failed to read session file: %w", err)
	}
	defer func() { _ = file.Close() }()

	var facts []importer.ImportedFact
	var project, model string

	scanner := bufio.NewScanner(file)
	// Increase buffer for large transcript lines.
	scanner.Buffer(make([]byte, 0, 64*1024), 1024*1024)

	for scanner.Scan() {
		line := scanner.Text()
		if strings.TrimSpace(line) == "" {
			continue
		}

		var entry ClaudeCodeEntry
		if err := json.Unmarshal([]byte(line), &entry); err != nil {
			continue // skip malformed lines
		}

		// Capture project/model from any entry that has them.
		if entry.Project != "" {
			project = entry.Project
		}
		if entry.Model != "" {
			model = entry.Model
		}

		// Extract assistant messages longer than 50 chars.
		if entry.Type == "assistant" || entry.Message.Role == "assistant" {
			content := entry.Message.Content
			if len(content) > 50 {
				ts := parseTimestamp(entry.Timestamp)

				facts = append(facts, importer.ImportedFact{
					Content:   content,
					Source:    "claude-code",
					SessionID: ref.SessionID,
					Timestamp: ts,
					Metadata: map[string]string{
						"project": project,
						"model":   model,
					},
				})
			}
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
