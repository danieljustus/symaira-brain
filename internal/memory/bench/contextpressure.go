// Package bench provides corpus-backed evaluation harnesses.
// The context-pressure benchmarks measure how the context assembler behaves
// under varying token budgets, retrieval loads, and boundary conditions.
package bench

import (
	"fmt"
	"strings"
	"sync"
)

// ---------------------------------------------------------------------------
// ContextPressureReport summarises a single context-pressure assessment.
// ---------------------------------------------------------------------------

// ContextPressureMode describes the pressure scenario being tested.
type ContextPressureMode string

const (
	PressureBudgetTight     ContextPressureMode = "budget_tight"      // very small token budget
	PressureBudgetGenerous  ContextPressureMode = "budget_generous"   // large token budget
	PressureManyResults     ContextPressureMode = "many_results"      // many retrieval results to fit
	PressureHighScoreSpread ContextPressureMode = "high_score_spread" // wide variance in retrieval scores
	PressureNoResults       ContextPressureMode = "no_results"        // empty retrieval
	PressureAllDegraded     ContextPressureMode = "all_degraded"      // everything must degrade
)

// ContextPressureReport measures how the assembler handles pressure scenarios.
type ContextPressureReport struct {
	Mode             ContextPressureMode `json:"mode"`
	Queries          int                 `json:"queries"`
	AvgPieces        float64             `json:"avg_pieces"`
	AvgUsedTokens    float64             `json:"avg_used_tokens"`
	AvgBudgetUtilPct float64             `json:"avg_budget_util_pct"`
	DegradationPct   float64             `json:"degradation_pct"` // fraction of pieces that were degraded
	MinPieces        int                 `json:"min_pieces"`
	MaxPieces        int                 `json:"max_pieces"`
	Description      string              `json:"description"`
}

// RecoveryCanaryReport captures recovery behaviour under stress.
type RecoveryCanaryReport struct {
	Scenario       string  `json:"scenario"`
	Recovered      bool    `json:"recovered"`       // did assembly complete without panic/error
	EmptyAssembly  bool    `json:"empty_assembly"`  // assembly produced zero pieces
	HadDegradation bool    `json:"had_degradation"` // degradation ladder was activated
	LatencyMs      float64 `json:"latency_ms"`
	Description    string  `json:"description"`
}

// ---------------------------------------------------------------------------
// ContextPressureSimulator simulates retrieval results at varying scales.
// ---------------------------------------------------------------------------

// SimulatedResult is a lightweight retrieval result for context-pressure testing.
type SimulatedResult struct {
	ID      string
	Content string
	Score   float32
}

// ResultProducer generates simulated retrieval results at a requested count.
type ResultProducer struct {
	mu       sync.Mutex
	rngState uint64
}

// NewResultProducer creates a deterministic result generator.
func NewResultProducer(seed uint64) *ResultProducer {
	return &ResultProducer{rngState: seed}
}

// Generate produces n results with a deterministic score distribution.
// scoreSpread controls variance: 0=uniform, 1=wide spread.
func (rp *ResultProducer) Generate(n int, scoreSpread float64) []SimulatedResult {
	rp.mu.Lock()
	defer rp.mu.Unlock()

	results := make([]SimulatedResult, n)
	for i := 0; i < n; i++ {
		rp.rngState = rp.rngState*6364136223846793005 + 1442695040888963407
		baseScore := 0.9 - (float64(i)/float64(n+1))*0.5 // 0.9 → 0.4 descending
		if scoreSpread > 0 {
			// Add some deterministic jitter
			jitter := (float64(rp.rngState%1000) / 1000.0) * scoreSpread * 0.3
			baseScore += jitter
		}
		if baseScore > 1.0 {
			baseScore = 1.0
		}
		if baseScore < 0.1 {
			baseScore = 0.1
		}
		contentLen := 50 + (i%10)*10 // varying content lengths
		results[i] = SimulatedResult{
			ID:      fmt.Sprintf("sim-%d", i),
			Content: strings.Repeat(fmt.Sprintf("word%d ", i), contentLen/5),
			Score:   float32(baseScore),
		}
	}
	return results
}

// ---------------------------------------------------------------------------
// Pressure scenario descriptors for reporting
// ---------------------------------------------------------------------------

func pressureDescription(mode ContextPressureMode) string {
	switch mode {
	case PressureBudgetTight:
		return "Tight token budget — each piece must degrade to reference to fit"
	case PressureBudgetGenerous:
		return "Generous token budget — all pieces fit at full level"
	case PressureManyResults:
		return "Many retrieval results — greedy fill with aggressive degradation"
	case PressureHighScoreSpread:
		return "Wide score spread — high-scoring items get full, low-scoring drop to reference"
	case PressureNoResults:
		return "Zero retrieval results — assembly must handle gracefully"
	case PressureAllDegraded:
		return "All results must degrade — extreme token pressure"
	default:
		return "Unknown pressure mode"
	}
}
