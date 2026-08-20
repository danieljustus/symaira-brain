package working

import (
	"context"
	"fmt"
	"log/slog"

	"github.com/danieljustus/symaira-brain/internal/memory/consolidation"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/extractor"
)

type CompactResult struct {
	ExpiredCount int64 `json:"expired_count"`
	EvictedCount int64 `json:"evicted_count"`
}

type Evictor struct {
	database   *db.DB
	embeddings *extractor.EmbeddingsGenerator
	engine     *consolidation.Engine
	piiEnabled bool
}

func NewEvictor(database *db.DB, embeddings *extractor.EmbeddingsGenerator, engine *consolidation.Engine, piiEnabled bool) *Evictor {
	return &Evictor{
		database:   database,
		embeddings: embeddings,
		engine:     engine,
		piiEnabled: piiEnabled,
	}
}

func (e *Evictor) CompactWorkingMemories(ctx context.Context, dryRun bool) (*CompactResult, error) {
	expired, err := e.database.GetExpiredWorkingMemories()
	if err != nil {
		return nil, fmt.Errorf("fetch expired working memories: %w", err)
	}

	result := &CompactResult{ExpiredCount: int64(len(expired))}

	if len(expired) == 0 {
		return result, nil
	}

	if len(expired) == 1 {
		if !dryRun {
			if err := e.database.DeleteMemory(expired[0].ID); err != nil {
				return nil, fmt.Errorf("evict expired working memory %s: %w", expired[0].ID, err)
			}
			result.EvictedCount = 1
			slog.Info("evicted expired working memory", "id", expired[0].ID)
		}
		return result, nil
	}

	consolidated, err := e.engine.RunConsolidationForMemories(ctx, expired, dryRun)
	if err != nil {
		return nil, fmt.Errorf("consolidate expired working memories: %w", err)
	}

	if !dryRun {
		evicted, err := e.database.EvictExpiredWorkingMemories()
		if err != nil {
			return nil, fmt.Errorf("evict expired working memories: %w", err)
		}
		result.EvictedCount = evicted
		slog.Info("compacted working memories", "expired", result.ExpiredCount, "evicted", evicted, "consolidated", len(consolidated))
	}

	return result, nil
}

func (e *Evictor) EvictExpired() (int64, error) {
	return e.database.EvictExpiredWorkingMemories()
}
