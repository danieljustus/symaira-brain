package aider

import (
	"bufio"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
)

// AiderImporter imports sessions from Aider.
type AiderImporter struct {
	customPaths []string
}

func NewAiderImporter(customPaths []string) *AiderImporter {
	return &AiderImporter{customPaths: customPaths}
}

func (a *AiderImporter) Name() string {
	return "aider"
}

func (a *AiderImporter) DiscoverSessions(since time.Time) ([]importer.SessionRef, error) {
	var sessions []importer.SessionRef

	paths := a.customPaths
	if len(paths) == 0 {
		home, err := os.UserHomeDir()
		if err != nil {
			return nil, err
		}
		paths = []string{home}
	}

	for _, basePath := range paths {
		err := filepath.Walk(basePath, func(path string, info os.FileInfo, err error) error {
			if err != nil {
				return nil
			}
			if info.IsDir() || filepath.Base(path) != ".aider.chat.history.md" {
				return nil
			}
			if info.ModTime().Before(since) {
				return nil
			}

			sessions = append(sessions, importer.SessionRef{
				Tool:       "aider",
				SessionID:  filepath.Dir(path),
				Path:       path,
				ModifiedAt: info.ModTime(),
				Metadata: map[string]string{
					"project": filepath.Dir(path),
				},
			})
			return nil
		})
		if err != nil {
			continue
		}
	}

	return sessions, nil
}

func (a *AiderImporter) ImportSession(ref importer.SessionRef) ([]importer.ImportedFact, error) {
	file, err := os.Open(ref.Path)
	if err != nil {
		return nil, fmt.Errorf("failed to open file: %w", err)
	}
	defer func() { _ = file.Close() }()

	var facts []importer.ImportedFact
	var currentRole string
	var currentContent strings.Builder

	scanner := bufio.NewScanner(file)
	for scanner.Scan() {
		line := scanner.Text()

		if strings.HasPrefix(line, "# ") || strings.HasPrefix(line, "## ") {
			if currentRole == "assistant" && currentContent.Len() > 50 {
				facts = append(facts, importer.ImportedFact{
					Content:   currentContent.String(),
					Source:    "aider",
					SessionID: ref.SessionID,
					Timestamp: ref.ModifiedAt,
					Metadata: map[string]string{
						"project": ref.Metadata["project"],
					},
				})
			}
			currentRole = ""
			currentContent.Reset()
			continue
		}

		if strings.HasPrefix(line, "**Human**:") || strings.HasPrefix(line, "**Assistant**:") {
			if currentRole == "assistant" && currentContent.Len() > 50 {
				facts = append(facts, importer.ImportedFact{
					Content:   currentContent.String(),
					Source:    "aider",
					SessionID: ref.SessionID,
					Timestamp: ref.ModifiedAt,
					Metadata: map[string]string{
						"project": ref.Metadata["project"],
					},
				})
			}
			if strings.HasPrefix(line, "**Human**:") {
				currentRole = "human"
			} else {
				currentRole = "assistant"
			}
			currentContent.Reset()
			continue
		}

		if currentRole != "" {
			currentContent.WriteString(line)
			currentContent.WriteString("\n")
		}
	}

	if currentRole == "assistant" && currentContent.Len() > 50 {
		facts = append(facts, importer.ImportedFact{
			Content:   currentContent.String(),
			Source:    "aider",
			SessionID: ref.SessionID,
			Timestamp: ref.ModifiedAt,
			Metadata: map[string]string{
				"project": ref.Metadata["project"],
			},
		})
	}

	return facts, nil
}
