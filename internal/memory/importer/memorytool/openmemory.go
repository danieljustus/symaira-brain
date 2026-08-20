package memorytool

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"
)

// OpenMemoryImporter imports memories from OpenMemory exports.
type OpenMemoryImporter struct{}

// OpenMemoryExport represents the OpenMemory export format.
type OpenMemoryExport struct {
	Memories []OpenMemoryMemory `json:"memories"`
	Facts    []OpenMemoryFact   `json:"facts"`
}

type OpenMemoryMemory struct {
	ID        string            `json:"id"`
	Content   string            `json:"content"`
	Metadata  map[string]string `json:"metadata"`
	CreatedAt string            `json:"created_at"`
}

type OpenMemoryFact struct {
	Subject    string  `json:"subject"`
	Predicate  string  `json:"predicate"`
	Object     string  `json:"object"`
	Confidence float64 `json:"confidence"`
}

func NewOpenMemoryImporter() *OpenMemoryImporter {
	return &OpenMemoryImporter{}
}

func (o *OpenMemoryImporter) Name() string {
	return "openmemory"
}

func (o *OpenMemoryImporter) DiscoverExports(path string) ([]ExportRef, error) {
	if path == "" {
		home, err := os.UserHomeDir()
		if err != nil {
			return nil, err
		}
		path = filepath.Join(home, ".openmemory")
	}

	info, err := os.Stat(path)
	if err != nil {
		return nil, err
	}

	var refs []ExportRef

	if !info.IsDir() {
		if strings.HasSuffix(path, ".json") {
			refs = append(refs, ExportRef{
				Tool:       "openmemory",
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
				Tool:       "openmemory",
				Path:       p,
				Format:     "json",
				ModifiedAt: info.ModTime(),
			})
		}
		return nil
	})

	return refs, err
}

func (o *OpenMemoryImporter) ImportExport(ref ExportRef) ([]ImportedFact, error) {
	data, err := os.ReadFile(ref.Path)
	if err != nil {
		return nil, fmt.Errorf("failed to read file: %w", err)
	}

	var export OpenMemoryExport
	if err := json.Unmarshal(data, &export); err != nil {
		return nil, fmt.Errorf("failed to parse JSON: %w", err)
	}

	var facts []ImportedFact

	for _, mem := range export.Memories {
		ts, _ := time.Parse(time.RFC3339, mem.CreatedAt)
		facts = append(facts, ImportedFact{
			Content:   mem.Content,
			Source:    "openmemory",
			Timestamp: ts,
			Metadata: map[string]interface{}{
				"openmemory_id": mem.ID,
			},
		})
	}

	for _, fact := range export.Facts {
		content := fmt.Sprintf("%s %s %s", fact.Subject, fact.Predicate, fact.Object)
		facts = append(facts, ImportedFact{
			Content:   content,
			Source:    "openmemory",
			Timestamp: time.Now(),
			Metadata: map[string]interface{}{
				"confidence": fact.Confidence,
			},
		})
	}

	return facts, nil
}
