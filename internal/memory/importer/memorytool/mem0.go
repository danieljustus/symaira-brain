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

// Mem0Importer imports memories from Mem0 exports.
type Mem0Importer struct{}

// Mem0Export represents the Mem0 export format.
type Mem0Export struct {
	Memories []Mem0Memory `json:"memories"`
}

type Mem0Memory struct {
	ID        string `json:"id"`
	Content   string `json:"content"`
	UserID    string `json:"user_id"`
	AgentID   string `json:"agent_id"`
	CreatedAt string `json:"created_at"`
}

func NewMem0Importer() *Mem0Importer {
	return &Mem0Importer{}
}

func (m *Mem0Importer) Name() string {
	return "mem0"
}

func (m *Mem0Importer) DiscoverExports(path string) ([]ExportRef, error) {
	if path == "" {
		home, err := os.UserHomeDir()
		if err != nil {
			return nil, err
		}
		path = filepath.Join(home, ".mem0")
	}

	info, err := os.Stat(path)
	if err != nil {
		return nil, err
	}

	var refs []ExportRef

	if !info.IsDir() {
		if strings.HasSuffix(path, ".json") {
			refs = append(refs, ExportRef{
				Tool:       "mem0",
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
		if !info.IsDir() && strings.HasSuffix(p, ".json") {
			refs = append(refs, ExportRef{
				Tool:       "mem0",
				Path:       p,
				Format:     "json",
				ModifiedAt: info.ModTime(),
			})
		}
		return nil
	})

	return refs, err
}

func (m *Mem0Importer) ImportExport(ref ExportRef) ([]ImportedFact, error) {
	data, err := os.ReadFile(ref.Path)
	if err != nil {
		return nil, fmt.Errorf("failed to read file: %w", err)
	}

	var export Mem0Export
	if err := json.Unmarshal(data, &export); err != nil {
		return nil, fmt.Errorf("failed to parse JSON: %w", err)
	}

	var facts []ImportedFact

	for _, mem := range export.Memories {
		ts, _ := time.Parse(time.RFC3339, mem.CreatedAt)

		metadata := map[string]interface{}{
			"mem0_id": mem.ID,
			// #483: coding-session material → code prompt family.
			consolidation.PromptModeKey: consolidation.PromptFamilyCode,
		}
		if mem.UserID != "" {
			metadata["user_id"] = mem.UserID
		}
		if mem.AgentID != "" {
			metadata["agent_id"] = mem.AgentID
		}

		facts = append(facts, ImportedFact{
			Content:   mem.Content,
			Source:    "mem0",
			Timestamp: ts,
			Metadata:  metadata,
		})
	}

	return facts, nil
}
