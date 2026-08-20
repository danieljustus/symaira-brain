package memorytool

import (
	"fmt"

	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
	"github.com/google/uuid"
)

// SmartImporter handles intelligent deduplication and linking during import.
type SmartImporter struct {
	database *db.DB
}

// NewSmartImporter creates a new smart importer.
func NewSmartImporter(database *db.DB) *SmartImporter {
	return &SmartImporter{database: database}
}

// ImportFacts intelligently imports facts with deduplication.
func (s *SmartImporter) ImportFacts(facts []ImportedFact, dryRun bool) (*ImportResult, error) {
	result := &ImportResult{}

	for _, fact := range facts {
		contentHash := db.ComputeContentHash(fact.Content)

		exists, err := s.database.FactExists(contentHash)
		if err != nil {
			return nil, fmt.Errorf("failed to check fact existence: %w", err)
		}

		if exists {
			result.Skipped++
			continue
		}

		if !dryRun {
			if err := s.storeFact(fact, contentHash); err != nil {
				return nil, fmt.Errorf("failed to store fact: %w", err)
			}
		}

		result.Created++
	}

	return result, nil
}

func (s *SmartImporter) storeFact(fact ImportedFact, contentHash string) error {
	metadata := make(map[string]string)
	for k, v := range fact.Metadata {
		metadata[fmt.Sprintf("%v", k)] = fmt.Sprintf("%v", v)
	}

	memory := &db.Memory{
		ID:                  uuid.New().String(),
		Content:             security.Redact(fact.Content),
		Scope:               "agent",
		Metadata:            security.RedactMap(metadata),
		ContentHash:         contentHash,
		CreatedAt:           fact.Timestamp,
		ConsolidationStatus: "raw",
	}

	return s.database.SaveMemory(memory)
}
