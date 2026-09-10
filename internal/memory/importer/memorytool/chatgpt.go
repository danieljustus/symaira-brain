package memorytool

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/consolidation"
)

const (
	// A ChatGPT export is JSON, but can contain arbitrarily large message text.
	// 64 MiB is large enough for ordinary exports while bounding parser memory.
	chatGPTMaxExportBytes      = 64 << 20
	chatGPTDiscoveryMaxEntries = 10_000
	chatGPTDiscoveryMaxDepth   = 16
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

func NewChatGPTImporter() *ChatGPTImporter { return &ChatGPTImporter{} }

func (c *ChatGPTImporter) Name() string { return "chatgpt" }

// DiscoverExports finds regular conversations.json files below path.
func (c *ChatGPTImporter) DiscoverExports(path string) ([]ExportRef, error) {
	if path == "" {
		home, err := os.UserHomeDir()
		if err != nil {
			return nil, err
		}
		path = filepath.Join(home, "Downloads")
	}
	return discoverChatGPTExports(path)
}

func (c *ChatGPTImporter) ImportExport(ref ExportRef) ([]ImportedFact, error) {
	data, err := readChatGPTExport(ref.Path)
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
			if msg.Message == nil || msg.Message.Author.Role != "assistant" {
				continue
			}
			content := strings.Join(msg.Message.Content.Parts, " ")
			if len(content) <= 50 {
				continue
			}
			facts = append(facts, ImportedFact{
				Content: content, Source: "chatgpt", Timestamp: ts,
				Metadata: map[string]interface{}{
					"title": export.Title,
					// #483: coding-session material → code prompt family.
					consolidation.PromptModeKey: consolidation.PromptFamilyCode,
				},
			})
		}
	}
	return facts, nil
}
