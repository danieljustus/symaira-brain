package memorytool

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"

	"github.com/danieljustus/symaira-brain/internal/memory/consolidation"
	"strings"
	"time"
)

// ChatGPTImporter imports memories from ChatGPT exports.
type ChatGPTImporter struct{}

// ChatGPTExport represents the ChatGPT conversations.json format.
type ChatGPTExport struct {
	Title      string                    `json:"title"`
	CreateTime float64                   `json:"create_time"`
	Mapping    map[string]ChatGPTMessage `json:"mapping"`
}

type ChatGPTMessage struct {
	Message *ChatGPTMessageContent `json:"message"`
}

type ChatGPTMessageContent struct {
	Author  ChatGPTAuthor  `json:"author"`
	Content ChatGPTContent `json:"content"`
}

type ChatGPTAuthor struct {
	Role string `json:"role"`
}

type ChatGPTContent struct {
	Parts []string `json:"parts"`
}

func NewChatGPTImporter() *ChatGPTImporter {
	return &ChatGPTImporter{}
}

func (c *ChatGPTImporter) Name() string {
	return "chatgpt"
}

func (c *ChatGPTImporter) DiscoverExports(path string) ([]ExportRef, error) {
	if path == "" {
		home, err := os.UserHomeDir()
		if err != nil {
			return nil, err
		}
		path = filepath.Join(home, "Downloads")
	}

	info, err := os.Stat(path)
	if err != nil {
		return nil, err
	}

	var refs []ExportRef

	if !info.IsDir() {
		if strings.HasSuffix(path, "conversations.json") {
			refs = append(refs, ExportRef{
				Tool:       "chatgpt",
				Path:       path,
				Format:     "json",
				ModifiedAt: info.ModTime(),
			})
		}
		return refs, nil
	}

	err = filepath.Walk(path, func(p string, info os.FileInfo, err error) error {
		if err != nil {
			return nil
		}
		if !info.IsDir() && strings.HasSuffix(p, "conversations.json") {
			refs = append(refs, ExportRef{
				Tool:       "chatgpt",
				Path:       p,
				Format:     "json",
				ModifiedAt: info.ModTime(),
			})
		}
		return nil
	})

	return refs, err
}

func (c *ChatGPTImporter) ImportExport(ref ExportRef) ([]ImportedFact, error) {
	data, err := os.ReadFile(ref.Path)
	if err != nil {
		return nil, fmt.Errorf("failed to read file: %w", err)
	}

	var exports []ChatGPTExport
	if err := json.Unmarshal(data, &exports); err != nil {
		return nil, fmt.Errorf("failed to parse JSON: %w", err)
	}

	var facts []ImportedFact

	for _, export := range exports {
		ts := time.Unix(int64(export.CreateTime), 0)

		for _, msg := range export.Mapping {
			if msg.Message == nil {
				continue
			}
			if msg.Message.Author.Role == "assistant" {
				content := strings.Join(msg.Message.Content.Parts, " ")
				if len(content) > 50 {
					facts = append(facts, ImportedFact{
						Content:   content,
						Source:    "chatgpt",
						Timestamp: ts,
						Metadata: map[string]interface{}{
							"title": export.Title,
							// #483: coding-session material → code prompt family.
							consolidation.PromptModeKey: consolidation.PromptFamilyCode,
						},
					})
				}
			}
		}
	}

	return facts, nil
}
