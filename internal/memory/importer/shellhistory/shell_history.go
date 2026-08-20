package shellhistory

import (
	"bufio"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
)

var (
	zshExtendedRe = regexp.MustCompile(`^: (\d+):\d+;(.+)`)
	bashRe        = regexp.MustCompile(`^#(\d+)$`)
)

var excludeCommands = map[string]bool{
	"cd": true, "ls": true, "pwd": true, "echo": true, "cat": true,
	"exit": true, "logout": true, "history": true, "clear": true,
}

var tagPrefixes = map[string]string{
	"brew":   "package-manager",
	"npm":    "package-manager",
	"yarn":   "package-manager",
	"pnpm":   "package-manager",
	"pip":    "package-manager",
	"uv":     "package-manager",
	"go":     "vcs",
	"git":    "vcs",
	"gh":     "github",
	"docker": "container",
	"make":   "build",
	"cargo":  "build",
	"cmake":  "build",
}

type ShellHistoryImporter struct {
	historyPath   string
	successOnly   bool
	filters       []string
	minDurationMs int
}

func NewShellHistoryImporter(historyPath string, successOnly bool, filters []string) *ShellHistoryImporter {
	if historyPath == "" {
		historyPath = detectHistoryPath()
	}
	return &ShellHistoryImporter{
		historyPath:   historyPath,
		successOnly:   successOnly,
		filters:       filters,
		minDurationMs: 1000,
	}
}

func detectHistoryPath() string {
	shell := os.Getenv("SHELL")
	if strings.Contains(shell, "zsh") {
		home, err := os.UserHomeDir()
		if err == nil {
			path := filepath.Join(home, ".zsh_history")
			if _, err := os.Stat(path); err == nil {
				return path
			}
		}
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return ""
	}
	return filepath.Join(home, ".bash_history")
}

func (s *ShellHistoryImporter) Name() string { return "shell-history" }

func (s *ShellHistoryImporter) Category() string { return "code" }

func (s *ShellHistoryImporter) PrivacyLevel() importer.PrivacyLevel {
	return importer.PrivacyConfidential
}

func (s *ShellHistoryImporter) RequiresPIIGuard() bool { return true }

func (s *ShellHistoryImporter) DiscoverSessions(since time.Time) ([]importer.SessionRef, error) {
	if s.historyPath == "" {
		return nil, fmt.Errorf("no shell history file found")
	}

	file, err := os.Open(s.historyPath)
	if err != nil {
		return nil, fmt.Errorf("failed to open history file: %w", err)
	}
	defer func() { _ = file.Close() }()

	var sessions []importer.SessionRef
	scanner := bufio.NewScanner(file)

	for scanner.Scan() {
		line := scanner.Text()
		if line == "" {
			continue
		}

		ts, cmd := parseHistoryLine(line)
		if ts.IsZero() || ts.Before(since) {
			continue
		}

		if excludeCommands[cmd] {
			continue
		}

		// Also check if the first word of the command is an excluded command.
		if first := strings.Fields(cmd); len(first) > 0 && excludeCommands[first[0]] {
			continue
		}

		if !s.matchesFilter(cmd) {
			continue
		}

		sessionID := strconv.FormatInt(ts.Unix(), 10)

		sessions = append(sessions, importer.SessionRef{
			Tool:       "shell-history",
			SessionID:  sessionID,
			Path:       s.historyPath,
			ModifiedAt: ts,
			Metadata: map[string]string{
				"command": cmd,
			},
		})
	}

	return sessions, scanner.Err()
}

func parseHistoryLine(line string) (time.Time, string) {
	if m := zshExtendedRe.FindStringSubmatch(line); m != nil {
		epoch, _ := strconv.ParseInt(m[1], 10, 64)
		return time.Unix(epoch, 0), m[2]
	}

	if m := bashRe.FindStringSubmatch(line); m != nil {
		epoch, _ := strconv.ParseInt(m[1], 10, 64)
		return time.Unix(epoch, 0), ""
	}

	return time.Time{}, line
}

func (s *ShellHistoryImporter) matchesFilter(cmd string) bool {
	if len(s.filters) == 0 {
		return true
	}
	for _, f := range s.filters {
		if strings.Contains(cmd, f) {
			return true
		}
	}
	return false
}

func (s *ShellHistoryImporter) ImportSession(ref importer.SessionRef) ([]importer.ImportedFact, error) {
	cmd := ref.Metadata["command"]
	if cmd == "" {
		return nil, nil
	}

	tag := tagCommand(cmd)
	content := fmt.Sprintf("User ran: %s", cmd)

	metadata := map[string]string{
		"command": cmd,
		"source":  "shell-history",
	}
	if tag != "" {
		metadata["tag"] = tag
	}

	return []importer.ImportedFact{{
		Content:   content,
		Source:    "shell-history",
		SessionID: ref.SessionID,
		Timestamp: ref.ModifiedAt,
		Metadata:  metadata,
	}}, nil
}

func tagCommand(cmd string) string {
	parts := strings.Fields(cmd)
	if len(parts) == 0 {
		return ""
	}
	bin := filepath.Base(parts[0])
	if tag, ok := tagPrefixes[bin]; ok {
		return tag
	}
	return ""
}
