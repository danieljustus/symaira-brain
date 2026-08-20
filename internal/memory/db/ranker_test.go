package db

import (
	"testing"
	"time"
)

func TestCompositeScore_EqualRelevance_RecentHigher(t *testing.T) {
	now := time.Now()
	yesterday := now.Add(-24 * time.Hour)
	weekAgo := now.Add(-7 * 24 * time.Hour)

	weights := RankingWeights{
		RelevanceWeight:           0.4,
		RecencyWeight:             0.3,
		ImportanceWeight:          0.3,
		AccessReinforcementWeight: 0.0,
		RecencyHalfLife:           30,
		AccessHalfLife:            14,
	}

	recent := CompositeScore(0.8, now, 0.5, 0, nil, nil, weights)
	yesterdayScore := CompositeScore(0.8, yesterday, 0.5, 0, nil, nil, weights)
	weekScore := CompositeScore(0.8, weekAgo, 0.5, 0, nil, nil, weights)

	if recent <= yesterdayScore {
		t.Errorf("expected recent (%f) > yesterday (%f)", recent, yesterdayScore)
	}
	if yesterdayScore <= weekScore {
		t.Errorf("expected yesterday (%f) > week ago (%f)", yesterdayScore, weekScore)
	}
}

func TestCompositeScore_HighImportanceRanks(t *testing.T) {
	now := time.Now()
	weights := RankingWeights{
		RelevanceWeight:           0.4,
		RecencyWeight:             0.3,
		ImportanceWeight:          0.3,
		AccessReinforcementWeight: 0.0,
		RecencyHalfLife:           30,
		AccessHalfLife:            14,
	}

	low := CompositeScore(0.8, now, 0.1, 0, nil, nil, weights)
	high := CompositeScore(0.8, now, 0.9, 0, nil, nil, weights)

	if high <= low {
		t.Errorf("expected high importance (%f) > low importance (%f)", high, low)
	}
}

func TestCompositeScore_RelevanceDominatesWhenWeightsFavor(t *testing.T) {
	now := time.Now()
	weights := RankingWeights{
		RelevanceWeight:           0.9,
		RecencyWeight:             0.05,
		ImportanceWeight:          0.05,
		AccessReinforcementWeight: 0.0,
		RecencyHalfLife:           30,
		AccessHalfLife:            14,
	}

	highRel := CompositeScore(0.95, now, 0.1, 0, nil, nil, weights)
	lowRel := CompositeScore(0.3, now, 0.9, 0, nil, nil, weights)

	if highRel <= lowRel {
		t.Errorf("expected high relevance (%f) > low relevance (%f)", highRel, lowRel)
	}
}

func TestRankSearchResults_Reorders(t *testing.T) {
	now := time.Now()
	weekAgo := now.Add(-7 * 24 * time.Hour)

	results := []SearchResult{
		{Memory: &Memory{UpdatedAt: weekAgo, Importance: 0.9}, Score: 0.5},
		{Memory: &Memory{UpdatedAt: now, Importance: 0.1}, Score: 0.8},
	}

	weights := RankingWeights{
		RelevanceWeight:           0.4,
		RecencyWeight:             0.3,
		ImportanceWeight:          0.3,
		AccessReinforcementWeight: 0.0,
		RecencyHalfLife:           30,
		AccessHalfLife:            14,
	}

	ranked := RankSearchResults(results, weights)
	if len(ranked) != 2 {
		t.Fatalf("expected 2 results, got %d", len(ranked))
	}
	// With weights 0.4/0.3/0.3, old/high-importance (0.5*0.4 + ~0.85*0.3 + 0.9*0.3 ≈ 0.73)
	// scores higher than recent/low-importance (0.8*0.4 + 1.0*0.3 + 0.1*0.3 ≈ 0.65),
	// so importance=0.9 ranks first.
	if ranked[0].Memory.Importance != 0.9 {
		t.Errorf("expected high-importance first with these weights, got importance=%f", ranked[0].Memory.Importance)
	}
}

func TestRankSearchResults_EmptyInput(t *testing.T) {
	weights := RankingWeights{}
	ranked := RankSearchResults(nil, weights)
	if ranked != nil {
		t.Errorf("expected nil output for nil input")
	}
}

func TestRankSearchResults_SingleResult(t *testing.T) {
	results := []SearchResult{
		{Memory: &Memory{UpdatedAt: time.Now(), Importance: 0.5}, Score: 0.8},
	}
	weights := RankingWeights{RelevanceWeight: 1.0}
	ranked := RankSearchResults(results, weights)
	if len(ranked) != 1 {
		t.Fatalf("expected 1 result, got %d", len(ranked))
	}
}
