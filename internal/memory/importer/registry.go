package importer

import (
	"context"
	"fmt"
	"os"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/extractor"
	"github.com/danieljustus/symaira-brain/internal/memory/memory"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
	"github.com/danieljustus/symaira-brain/internal/memory/summarizer"
	"github.com/danieljustus/symaira-corekit/evidencekit"
	"github.com/google/uuid"
)

// Registry manages multiple session importers and orchestrates import runs.
type Registry struct {
	importers       map[string]SessionImporter
	database        *db.DB
	embeddings      *extractor.EmbeddingsGenerator
	state           *ImportState
	extractOnImport bool
	stderr          *os.File
}

// NewRegistry creates a new importer registry.
func NewRegistry(database *db.DB, embeddings *extractor.EmbeddingsGenerator, extractOnImport bool) *Registry {
	return &Registry{
		importers:       make(map[string]SessionImporter),
		database:        database,
		embeddings:      embeddings,
		state:           NewImportState(database.Conn()),
		extractOnImport: extractOnImport,
		stderr:          os.Stderr,
	}
}

// Register adds a session importer to the registry.
func (r *Registry) Register(importer SessionImporter) {
	r.importers[importer.Name()] = importer
}

// Get returns an importer by name, or nil if not found.
func (r *Registry) Get(name string) SessionImporter {
	return r.importers[name]
}

// List returns all registered importer names.
func (r *Registry) List() []string {
	names := make([]string, 0, len(r.importers))
	for name := range r.importers {
		names = append(names, name)
	}
	return names
}

// ListByCategory returns importer names filtered by category.
// Only importers implementing Categorizable are matched.
func (r *Registry) ListByCategory(category string) []string {
	var names []string
	for _, imp := range r.importers {
		if cat, ok := imp.(Categorizable); ok && cat.Category() == category {
			names = append(names, imp.Name())
		}
	}
	return names
}

// ListByPrivacyLevel returns importer names whose privacy level is at or below
// the specified threshold. Only importers implementing PrivacyAware are matched.
func (r *Registry) ListByPrivacyLevel(maxLevel PrivacyLevel) []string {
	levels := map[PrivacyLevel]int{
		PrivacyPublic:       0,
		PrivacyInternal:     1,
		PrivacyConfidential: 2,
		PrivacySecret:       3,
	}
	max := levels[maxLevel]

	var names []string
	for _, imp := range r.importers {
		if pa, ok := imp.(PrivacyAware); ok {
			if levels[pa.PrivacyLevel()] <= max {
				names = append(names, imp.Name())
			}
		} else {
			names = append(names, imp.Name())
		}
	}
	return names
}

// ImportResult tracks the outcome of an import run.
type ImportResult struct {
	Tool     string
	Sessions int
	Facts    int
	Skipped  int
	Errors   int
	DryRun   bool
}

// RunImport runs import for specified tools (or all if empty).
func (r *Registry) RunImport(ctx context.Context, tools []string, dryRun bool) ([]ImportResult, error) {
	if len(tools) == 0 {
		tools = r.List()
	}

	var results []ImportResult

	for _, toolName := range tools {
		importer := r.importers[toolName]
		if importer == nil {
			return nil, fmt.Errorf("unknown importer %q; valid importers: %s", toolName, strings.Join(r.List(), ", "))
		}

		result, err := r.runToolImport(ctx, importer, dryRun)
		if err != nil {
			fmt.Fprintf(r.stderr, "Error importing from %s: %v\n", toolName, err)
			result = &ImportResult{Tool: toolName, Errors: 1, DryRun: dryRun}
		}
		results = append(results, *result)
	}

	return results, nil
}

// runToolImport runs import for a single tool.
func (r *Registry) runToolImport(ctx context.Context, importer SessionImporter, dryRun bool) (*ImportResult, error) {
	result := &ImportResult{
		Tool:   importer.Name(),
		DryRun: dryRun,
	}

	// Get last import time for incremental discovery
	lastImport, err := r.state.GetLastImportTime(importer.Name())
	if err != nil {
		return nil, fmt.Errorf("failed to get last import time: %w", err)
	}

	// If no previous import, start from 30 days ago
	if lastImport.IsZero() {
		lastImport = time.Now().AddDate(0, 0, -30)
	}

	// Discover sessions
	sessions, err := importer.DiscoverSessions(lastImport)
	if err != nil {
		return nil, fmt.Errorf("failed to discover sessions: %w", err)
	}

	fmt.Fprintf(r.stderr, "Found %d sessions from %s\n", len(sessions), importer.Name())

	for _, session := range sessions {
		select {
		case <-ctx.Done():
			return result, ctx.Err()
		default:
		}

		// Check if already imported
		imported, err := r.state.IsImported(importer.Name(), session.SessionID)
		if err != nil {
			fmt.Fprintf(r.stderr, "Error checking import state for %s: %v\n", session.SessionID, err)
			result.Errors++
			continue
		}

		if imported {
			result.Skipped++
			continue
		}

		// Import the session
		facts, err := importer.ImportSession(session)
		if err != nil {
			fmt.Fprintf(r.stderr, "Error importing session %s: %v\n", session.SessionID, err)
			result.Errors++
			continue
		}

		if r.extractOnImport {
			if _, isTranscript := importer.(TranscriptImporter); isTranscript {
				facts = r.extractFacts(facts)
			}
		} else {
			if _, isTranscript := importer.(TranscriptImporter); isTranscript {
				fmt.Fprintf(r.stderr, "Extraction disabled for %s; storing raw facts\n", importer.Name())
			}
		}

		result.Sessions++
		result.Facts += len(facts)

		if !dryRun {
			// Store facts in database
			if err := r.storeFacts(facts, importer); err != nil {
				fmt.Fprintf(r.stderr, "Error storing facts for session %s: %v\n", session.SessionID, err)
				result.Errors++
				continue
			}

			// Mark as imported
			if err := r.state.MarkImported(importer.Name(), session.SessionID, len(facts)); err != nil {
				fmt.Fprintf(r.stderr, "Error marking session %s as imported: %v\n", session.SessionID, err)
				result.Errors++
			}
		}
	}

	return result, nil
}

// storeFacts stores imported facts in the database with local embeddings.
func (r *Registry) storeFacts(facts []ImportedFact, importer SessionImporter) error {
	for _, fact := range facts {
		now := time.Now().UTC()

		metadata := fact.Metadata
		if metadata == nil {
			metadata = make(map[string]string)
		}

		sourceURI := ""
		if _, ok := metadata["session_id"]; ok {
			sourceURI = metadata["session_id"]
		}
		provenance := memory.ImportedProvenance(fact.Source, sourceURI, fact.Timestamp)
		metadata = memory.MergeProvenance(metadata, provenance)

		if pa, ok := importer.(PrivacyAware); ok {
			privacyLevel := string(pa.PrivacyLevel())
			if privacyLevel != "" {
				metadata[memory.MetaSensitivity] = privacyLevel
			}
		}

		trustDefaults := memory.ImportedTrustMetadata()
		metadata = memory.MergeProvenance(metadata, trustDefaults)

		if _, ok := metadata[memory.MetaSharingLevel]; !ok {
			metadata[memory.MetaSharingLevel] = memory.SharingPrivate
		}

		content := security.Redact(fact.Content)

		memory := &db.Memory{
			ID:                  uuid.New().String(),
			Content:             content,
			Scope:               "agent",
			Metadata:            security.RedactMap(metadata),
			CreatedAt:           fact.Timestamp,
			UpdatedAt:           now,
			ConsolidationStatus: "raw",
		}

		if r.embeddings != nil {
			emb := r.embeddings.GenerateVector(content)
			memory.Embedding = emb.Vector
			memory.EmbeddingSource = emb.Source
			memory.EmbeddingModel = emb.Model
			memory.EmbeddingQuantization = emb.Quantization
		}

		if err := r.database.SaveMemory(memory); err != nil {
			return fmt.Errorf("failed to save memory: %w", err)
		}

		if len(fact.Evidence) > 0 {
			redacted := make([]evidencekit.Extraction, len(fact.Evidence))
			for i, ext := range fact.Evidence {
				ext.EvidenceText = security.Redact(ext.EvidenceText)
				ext.Text = security.Redact(ext.Text)
				redacted[i] = ext
			}
			if err := r.database.SaveMemoryEvidence(memory.ID, redacted); err != nil {
				return fmt.Errorf("failed to save memory evidence: %w", err)
			}
		}
	}
	return nil
}

// extractFacts runs offline extraction (pattern matching + extractive summarization)
// on a batch of raw transcript facts, returning distilled ImportedFacts.
// If extraction produces no results, the original raw facts are returned unchanged.
func (r *Registry) extractFacts(facts []ImportedFact) []ImportedFact {
	if len(facts) == 0 {
		return facts
	}

	var transcript strings.Builder
	for _, f := range facts {
		if f.Content != "" {
			transcript.WriteString(f.Content)
			transcript.WriteString("\n")
		}
	}
	combined := strings.TrimSpace(transcript.String())
	if combined == "" {
		return facts
	}

	pe := extractor.NewPatternExtractor()
	es := summarizer.NewExtractiveSummarizer()

	extracted := pe.ExtractFacts(combined)
	summary := es.SummarizeSession(combined, 5)

	if len(extracted) == 0 && summary == "" {
		return facts
	}

	first := facts[0]
	var distilled []ImportedFact

	for _, ef := range extracted {
		evidence := make([]evidencekit.Extraction, len(ef.Evidence))
		for i, ext := range ef.Evidence {
			ext.Source = evidencekit.SourceRef{ID: first.SessionID, Kind: first.Source}
			evidence[i] = ext
		}
		distilled = append(distilled, ImportedFact{
			Content:   ef.Content,
			Source:    first.Source,
			SessionID: first.SessionID,
			Timestamp: first.Timestamp,
			Metadata:  mergeMetadata(first.Metadata, ef.Metadata),
			Evidence:  evidence,
		})
	}

	if summary != "" {
		distilled = append(distilled, ImportedFact{
			Content:   summary,
			Source:    first.Source,
			SessionID: first.SessionID,
			Timestamp: first.Timestamp,
			Metadata: map[string]string{
				"method":       "extractive_summarization",
				"fact_count":   fmt.Sprintf("%d", len(facts)),
				"distilled_to": fmt.Sprintf("%d", len(distilled)),
			},
		})
	}

	return distilled
}

func mergeMetadata(base, extra map[string]string) map[string]string {
	merged := make(map[string]string, len(base)+len(extra))
	for k, v := range base {
		merged[k] = v
	}
	for k, v := range extra {
		merged[k] = v
	}
	return merged
}
