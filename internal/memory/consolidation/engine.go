package consolidation

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"regexp"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/extractor"
	"github.com/danieljustus/symaira-brain/internal/memory/llm"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
	"github.com/google/uuid"
)

// ConsolidationResult represents the parsed response from the LLM.
type ConsolidationResult struct {
	Consolidated []ConsolidatedItem `json:"consolidated"`
	DiscardedIDs []string           `json:"discarded_ids"`
}

// ConsolidatedItem represents a newly synthesized memory fact.
type ConsolidatedItem struct {
	Content     string            `json:"content"`
	ReplacesIDs []string          `json:"replaces_ids"`
	Metadata    map[string]string `json:"metadata"`
}

// ScopeChangeSummary captures the actions taken or planned for a single scope.
type ScopeChangeSummary struct {
	Scope             string
	NewMemories       []*db.Memory
	ArchivedMemoryIDs []string
	ReplacedIDToNewID map[string]string
}

// Engine orchestrates the memory consolidation process.
type Engine struct {
	database    *db.DB
	embeddings  *extractor.EmbeddingsGenerator
	llmClient   *llm.Client
	llmProvider string
	piiEnabled  bool
	promptMode  string // #483: default prompt family ("chat" | "code")
}

// NewEngine creates a new consolidation engine instance. promptMode is the
// configured default prompt family (#483): "chat" or "code"; per-group
// metadata overrides it (see ResolveFamily).
func NewEngine(database *db.DB, embeddings *extractor.EmbeddingsGenerator, llmURL, llmModel, llmProvider string, piiEnabled bool, promptMode string, llmTimeout time.Duration) *Engine {
	if llmProvider == "" {
		if apiKey := os.Getenv("OPENAI_API_KEY"); apiKey != "" {
			llmProvider = "openai"
		} else {
			llmProvider = "ollama"
		}
	}

	return &Engine{
		database:    database,
		embeddings:  embeddings,
		llmClient:   llm.NewClient(llmURL, llmModel, llmProvider, llmTimeout),
		llmProvider: llmProvider,
		piiEnabled:  piiEnabled,
		promptMode:  promptMode,
	}
}

// scopeFailure records a non-fatal per-scope LLM/parse failure during a
// consolidation run.
type scopeFailure struct {
	Scope string
	Err   error
}

// reportScopeFailures logs skipped scopes with diagnosable detail and returns
// a hard error only if every scope in the run failed.
func reportScopeFailures(failures []scopeFailure, succeeded int) error {
	for _, f := range failures {
		fmt.Fprintf(os.Stderr, "consolidation: skipping scope %q after LLM failure: %v\n", f.Scope, f.Err)
	}
	fmt.Fprintf(os.Stderr, "consolidation: run summary: %d scope(s) succeeded, %d scope(s) skipped due to LLM failures\n", succeeded, len(failures))
	if succeeded == 0 && len(failures) > 0 {
		return fmt.Errorf("consolidation failed: all scopes failed (%d): first error for scope %q: %w", len(failures), failures[0].Scope, failures[0].Err)
	}
	return nil
}

// RunConsolidation finds raw memories, groups them by scope, prompts the LLM,
// and applies the changes inside a database transaction (unless dryRun is true).
func (eng *Engine) RunConsolidation(ctx context.Context, scopeFilter string, dryRun bool) ([]ScopeChangeSummary, error) {
	rawMemories, err := eng.database.GetRawMemories()
	if err != nil {
		return nil, fmt.Errorf("failed to fetch raw memories: %w", err)
	}

	if len(rawMemories) == 0 {
		return nil, nil
	}

	// Group raw memories by scope
	grouped := make(map[string][]*db.Memory)
	for _, m := range rawMemories {
		if scopeFilter != "" && m.Scope != scopeFilter {
			continue
		}
		grouped[m.Scope] = append(grouped[m.Scope], m)
	}

	var summaries []ScopeChangeSummary
	var failures []scopeFailure

	for scope, memories := range grouped {
		summary := ScopeChangeSummary{
			Scope:             scope,
			ReplacedIDToNewID: make(map[string]string),
		}

		// If there is only one raw memory, we don't need LLM consolidation,
		// we can simply mark it as consolidated.
		if len(memories) <= 1 {
			m := memories[0]
			updatedMemory := *m
			updatedMemory.ConsolidationStatus = "consolidated"
			updatedMemory.UpdatedAt = time.Now().UTC()

			summary.NewMemories = append(summary.NewMemories, &updatedMemory)
			summary.ArchivedMemoryIDs = append(summary.ArchivedMemoryIDs, m.ID)
			summary.ReplacedIDToNewID[m.ID] = m.ID

			if !dryRun {
				tx, err := eng.database.BeginTransaction()
				if err != nil {
					return nil, fmt.Errorf("failed to begin transaction: %w", err)
				}
				if err := eng.database.SaveMemoryTx(tx, &updatedMemory); err != nil {
					_ = tx.Rollback()
					return nil, fmt.Errorf("failed to update raw memory to consolidated: %w", err)
				}
				if err := tx.Commit(); err != nil {
					return nil, fmt.Errorf("failed to commit transaction: %w", err)
				}
			}
			summaries = append(summaries, summary)
			continue
		}

		// Prompt the LLM for consolidation
		res, err := eng.consolidateWithLLM(ctx, scope, memories)
		if err != nil {
			// If LLM fails, log it and skip this scope, allowing subsequent
			// scopes to proceed. A hard error is surfaced only when every
			// scope in the run failed.
			failures = append(failures, scopeFailure{Scope: scope, Err: err})
			continue
		}

		// Process results
		txMemMap := make(map[string]*db.Memory)

		for _, item := range res.Consolidated {
			content := item.Content
			if eng.piiEnabled {
				content = security.Redact(content)
			}

			// Generate vector embedding
			var vector []float32
			var embSource, embModel string
			if eng.embeddings != nil {
				emb := eng.embeddings.GenerateVector(content)
				vector = emb.Vector
				embSource = emb.Source
				embModel = emb.Model
			}

			newID := uuid.New().String()
			now := time.Now().UTC()

			meta := item.Metadata
			if meta == nil {
				meta = make(map[string]string)
			}
			// Provenance (#493): the synthesized fact was derived from
			// stored/ingested content that is untrusted by default.
			meta[security.UntrustedContentKey] = "true"

			newMem := &db.Memory{
				ID:                  newID,
				Content:             content,
				Scope:               scope,
				Metadata:            meta,
				Embedding:           vector,
				EmbeddingSource:     embSource,
				EmbeddingModel:      embModel,
				CreatedAt:           now,
				UpdatedAt:           now,
				ConsolidationStatus: "consolidated",
			}

			summary.NewMemories = append(summary.NewMemories, newMem)
			txMemMap[newID] = newMem

			for _, replID := range item.ReplacesIDs {
				summary.ArchivedMemoryIDs = append(summary.ArchivedMemoryIDs, replID)
				summary.ReplacedIDToNewID[replID] = newID
			}
		}

		summary.ArchivedMemoryIDs = append(summary.ArchivedMemoryIDs, res.DiscardedIDs...)

		if !dryRun {
			tx, err := eng.database.BeginTransaction()
			if err != nil {
				return nil, fmt.Errorf("failed to begin transaction: %w", err)
			}

			// Save new memories
			for _, m := range summary.NewMemories {
				if err := eng.database.SaveMemoryTx(tx, m); err != nil {
					_ = tx.Rollback()
					return nil, fmt.Errorf("failed to save consolidated memory: %w", err)
				}
			}

			// Propagate evidence from replaced raw memories to their new
			// consolidated memory so grounding is preserved across merges.
			for oldID, newID := range summary.ReplacedIDToNewID {
				if err := eng.database.ReparentMemoryEvidenceTx(tx, oldID, newID); err != nil {
					_ = tx.Rollback()
					return nil, fmt.Errorf("failed to propagate evidence from %s to %s: %w", oldID, newID, err)
				}
			}

			// Update replaced memories to archived with parent link
			for _, m := range memories {
				status := "archived"
				parentID := summary.ReplacedIDToNewID[m.ID]

				isDiscarded := false
				for _, discID := range res.DiscardedIDs {
					if discID == m.ID {
						isDiscarded = true
						break
					}
				}

				isReplaced := parentID != ""

				// If not explicitly replaced or discarded by LLM, let's keep it raw
				// to avoid losing data due to LLM parsing/omission issues.
				if !isReplaced && !isDiscarded {
					continue
				}

				if isDiscarded {
					parentID = ""
				}

				if err := eng.database.UpdateMemoryStatusTx(tx, m.ID, status, parentID); err != nil {
					_ = tx.Rollback()
					return nil, fmt.Errorf("failed to archive original memory %s: %w", m.ID, err)
				}
			}

			if err := tx.Commit(); err != nil {
				return nil, fmt.Errorf("failed to commit transaction: %w", err)
			}
		}

		summaries = append(summaries, summary)
	}

	if err := reportScopeFailures(failures, len(summaries)); err != nil {
		return nil, err
	}

	// Journal the run for undo support (no-op when dryRun or empty).
	eng.journalRun(dryRun, summaries)

	return summaries, nil
}

// RunConsolidationForMemories consolidates the provided list of memories.
// Unlike RunConsolidation which fetches raw memories from the DB, this method
// works on the caller-supplied list (used for working-memory compaction).
func (eng *Engine) RunConsolidationForMemories(ctx context.Context, memories []*db.Memory, dryRun bool) ([]ScopeChangeSummary, error) {
	if len(memories) == 0 {
		return nil, nil
	}

	grouped := make(map[string][]*db.Memory)
	for _, m := range memories {
		grouped[m.Scope] = append(grouped[m.Scope], m)
	}

	var summaries []ScopeChangeSummary
	var failures []scopeFailure

	for scope, scopeMemories := range grouped {
		summary := ScopeChangeSummary{
			Scope:             scope,
			ReplacedIDToNewID: make(map[string]string),
		}

		if len(scopeMemories) <= 1 {
			m := scopeMemories[0]
			updatedMemory := *m
			updatedMemory.ConsolidationStatus = "consolidated"
			updatedMemory.UpdatedAt = time.Now().UTC()

			summary.NewMemories = append(summary.NewMemories, &updatedMemory)
			summary.ArchivedMemoryIDs = append(summary.ArchivedMemoryIDs, m.ID)
			summary.ReplacedIDToNewID[m.ID] = m.ID

			if !dryRun {
				tx, err := eng.database.BeginTransaction()
				if err != nil {
					return nil, fmt.Errorf("failed to begin transaction: %w", err)
				}
				if err := eng.database.SaveMemoryTx(tx, &updatedMemory); err != nil {
					_ = tx.Rollback()
					return nil, fmt.Errorf("failed to update raw memory to consolidated: %w", err)
				}
				if err := tx.Commit(); err != nil {
					return nil, fmt.Errorf("failed to commit transaction: %w", err)
				}
			}
			summaries = append(summaries, summary)
			continue
		}

		res, err := eng.consolidateWithLLM(ctx, scope, scopeMemories)
		if err != nil {
			// Skip this scope on LLM failure and continue with the rest; a
			// hard error is surfaced only when every scope failed.
			failures = append(failures, scopeFailure{Scope: scope, Err: err})
			continue
		}

		txMemMap := make(map[string]*db.Memory)

		for _, item := range res.Consolidated {
			content := item.Content
			if eng.piiEnabled {
				content = security.Redact(content)
			}

			var vector []float32
			var embSource, embModel string
			if eng.embeddings != nil {
				emb := eng.embeddings.GenerateVector(content)
				vector = emb.Vector
				embSource = emb.Source
				embModel = emb.Model
			}

			newID := uuid.New().String()
			now := time.Now().UTC()

			meta := item.Metadata
			if meta == nil {
				meta = make(map[string]string)
			}
			// Provenance (#493): the synthesized fact was derived from
			// stored/ingested content that is untrusted by default.
			meta[security.UntrustedContentKey] = "true"

			newMem := &db.Memory{
				ID:                  newID,
				Content:             content,
				Scope:               scope,
				Metadata:            meta,
				Embedding:           vector,
				EmbeddingSource:     embSource,
				EmbeddingModel:      embModel,
				CreatedAt:           now,
				UpdatedAt:           now,
				ConsolidationStatus: "consolidated",
			}

			summary.NewMemories = append(summary.NewMemories, newMem)
			txMemMap[newID] = newMem

			for _, replID := range item.ReplacesIDs {
				summary.ArchivedMemoryIDs = append(summary.ArchivedMemoryIDs, replID)
				summary.ReplacedIDToNewID[replID] = newID
			}
		}

		summary.ArchivedMemoryIDs = append(summary.ArchivedMemoryIDs, res.DiscardedIDs...)

		if !dryRun {
			tx, err := eng.database.BeginTransaction()
			if err != nil {
				return nil, fmt.Errorf("failed to begin transaction: %w", err)
			}

			for _, m := range summary.NewMemories {
				if err := eng.database.SaveMemoryTx(tx, m); err != nil {
					_ = tx.Rollback()
					return nil, fmt.Errorf("failed to save consolidated memory: %w", err)
				}
			}

			for oldID, newID := range summary.ReplacedIDToNewID {
				if err := eng.database.ReparentMemoryEvidenceTx(tx, oldID, newID); err != nil {
					_ = tx.Rollback()
					return nil, fmt.Errorf("failed to propagate evidence from %s to %s: %w", oldID, newID, err)
				}
			}

			for _, m := range scopeMemories {
				status := "archived"
				parentID := summary.ReplacedIDToNewID[m.ID]

				isDiscarded := false
				for _, discID := range res.DiscardedIDs {
					if discID == m.ID {
						isDiscarded = true
						break
					}
				}

				isReplaced := parentID != ""
				if !isReplaced && !isDiscarded {
					continue
				}

				if isDiscarded {
					parentID = ""
				}

				if err := eng.database.UpdateMemoryStatusTx(tx, m.ID, status, parentID); err != nil {
					_ = tx.Rollback()
					return nil, fmt.Errorf("failed to archive original memory %s: %w", m.ID, err)
				}
			}

			if err := tx.Commit(); err != nil {
				return nil, fmt.Errorf("failed to commit transaction: %w", err)
			}
		}

		summaries = append(summaries, summary)
	}

	if err := reportScopeFailures(failures, len(summaries)); err != nil {
		return nil, err
	}

	// Journal the run for undo support (no-op when dryRun or empty).
	eng.journalRun(dryRun, summaries)

	return summaries, nil
}

// memoryIndex maps a short integer index (1..N) to its memory for the LLM
// prompt. Using integers instead of UUIDs reduces token usage and prevents the
// LLM from hallucinating or inventing synthetic UUIDs.
type memoryIndex struct {
	mem  *db.Memory
	uuid string // original UUID
}

// buildMemoryPrompt formats memories with short integer indices and returns
// the prompt string plus the reverse mapping (index → UUID). Every memory's
// content is treated as untrusted data (#493): instruction-injection
// markers are neutralized line-wise before the block is composed.
func buildMemoryPrompt(scope string, memories []*db.Memory) (string, []memoryIndex) {
	var builder strings.Builder
	builder.WriteString("<memory_content>\n")
	indices := make([]memoryIndex, len(memories))
	for i, m := range memories {
		idx := i + 1
		indices[i] = memoryIndex{mem: m, uuid: m.ID}
		content := security.SanitizeLines(m.Content)
		fmt.Fprintf(&builder, "- [%d] Content: %s\n  Created: %s\n", idx, content, m.CreatedAt.Format(time.RFC3339))
	}
	builder.WriteString("</memory_content>")
	return builder.String(), indices
}

// mapLLMIDxToUUID converts slice-of-string index references from the LLM
// response back to UUIDs using the indices slice. Returns the mapped slice.
// Indices are 1-based. Unknown indices are silently dropped.
func mapLLMIDxToUUID(refs []string, indices []memoryIndex) []string {
	// Build a lookup from the indices slice. The struct doesn't carry idx so
	// we use position: index i+1 maps to indices[i].uuid.
	idxToUUID := make(map[string]string, len(indices))
	for i, entry := range indices {
		idxToUUID[fmt.Sprintf("%d", i+1)] = entry.uuid
	}
	out := make([]string, 0, len(refs))
	for _, r := range refs {
		if uuid, ok := idxToUUID[r]; ok {
			out = append(out, uuid)
		}
	}
	return out
}

func (eng *Engine) consolidateWithLLM(ctx context.Context, scope string, memories []*db.Memory) (*ConsolidationResult, error) {
	promptContent, indices := buildMemoryPrompt(scope, memories)

	// #483: resolve the prompt family for this group (per-group metadata
	// majority overrides the configured default) and build the prompts
	// through the family builders.
	family := ResolveFamily(memories, eng.promptMode)
	systemPrompt := BuildSystemPrompt(family)
	userPrompt := BuildUserPrompt(family, scope, promptContent)

	var rawResponse string
	var err error

	rawResponse, err = eng.llmClient.Query(ctx, systemPrompt, userPrompt, eng.llmProvider)

	if err != nil {
		return nil, err
	}

	res, err := parseJSONResponse(rawResponse)
	if err != nil {
		return nil, err
	}

	// Map short integer indices back to real UUIDs
	for i, item := range res.Consolidated {
		res.Consolidated[i].ReplacesIDs = mapLLMIDxToUUID(item.ReplacesIDs, indices)
	}
	res.DiscardedIDs = mapLLMIDxToUUID(res.DiscardedIDs, indices)

	return res, nil
}

var (
	// fencedBlockRe matches a Markdown code fence (``` or ~~~, optional
	// language tag) anywhere in the response text and captures its content.
	fencedBlockRe = regexp.MustCompile("(?s)```[^\\n`]*\\n(.*?)```")
	// thinkBlockRe matches a reasoning-model <think>...</think> preamble.
	thinkBlockRe = regexp.MustCompile("(?s)<think>.*?</think>")
)

func parseJSONResponse(rawResponse string) (*ConsolidationResult, error) {
	// Build an ordered list of candidate payloads, most specific first.
	seen := make(map[string]bool)
	var candidates []string
	addCandidate := func(s string) {
		s = strings.TrimSpace(s)
		if s != "" && !seen[s] {
			seen[s] = true
			candidates = append(candidates, s)
		}
	}

	cleaned := strings.TrimSpace(rawResponse)

	// First attempt: the raw response as-is. Fence extraction and
	// <think>...</think> stripping are only applied as retries after this
	// direct parse fails.
	addCandidate(cleaned)

	// Regex-based fence extraction: find a fenced JSON block anywhere in the
	// response, even when surrounded by prose before or after the fence.
	if m := fencedBlockRe.FindStringSubmatch(cleaned); m != nil {
		addCandidate(m[1])
	}

	// Fallback: the outermost {...} span, tolerating surrounding prose.
	addBraceSpan := func(s string) {
		if start := strings.Index(s, "{"); start >= 0 {
			if end := strings.LastIndex(s, "}"); end > start {
				addCandidate(s[start : end+1])
			}
		}
	}
	addBraceSpan(cleaned)

	// Strip-and-retry for reasoning-model <think>...</think> preambles.
	if thinkBlockRe.MatchString(cleaned) {
		stripped := strings.TrimSpace(thinkBlockRe.ReplaceAllString(cleaned, ""))
		if m := fencedBlockRe.FindStringSubmatch(stripped); m != nil {
			addCandidate(m[1])
		}
		addCandidate(stripped)
		addBraceSpan(stripped)
	}

	var lastErr error
	for _, candidate := range candidates {
		var res ConsolidationResult
		if err := json.Unmarshal([]byte(candidate), &res); err != nil {
			lastErr = err
			continue
		}
		if err := validateConsolidationResult(&res); err != nil {
			return nil, fmt.Errorf("consolidation result validation failed: %w", err)
		}
		return &res, nil
	}

	return nil, fmt.Errorf("failed to parse consolidation result JSON: %w (raw response: %s)", lastErr, rawResponse)
}

func validateConsolidationResult(res *ConsolidationResult) error {
	for i, item := range res.Consolidated {
		if strings.TrimSpace(item.Content) == "" {
			return fmt.Errorf("consolidated item %d has empty content", i)
		}
		if item.ReplacesIDs == nil {
			item.ReplacesIDs = []string{}
		}
	}
	if res.DiscardedIDs == nil {
		res.DiscardedIDs = []string{}
	}
	return nil
}
