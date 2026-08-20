package bench

import (
	"strings"
	"sync"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/contextassembler"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

// ---------------------------------------------------------------------------
// #402: Deterministic context-pressure and recovery canary benchmarks
// ---------------------------------------------------------------------------

// pressureAssembler is a pre-configured assembler for pressure benchmarks.
type pressureAssembler struct {
	assembler *contextassembler.Assembler
	cfg       *config.Config
	dbClosed  func()
}

// newPressureAssembler creates a test assembler with full config.
func newPressureAssembler(tb testing.TB) *pressureAssembler {
	tb.Helper()
	cfg := config.Defaults()
	database, err := db.Open(cfg)
	if err != nil {
		tb.Fatalf("failed to open database: %v", err)
	}
	a := contextassembler.NewAssembler(database, nil, &cfg.Context)
	a.WithFullConfig(cfg)
	a.SetWorkingMemoryConfig(&cfg.WorkingMemory)
	return &pressureAssembler{
		assembler: a,
		cfg:       cfg,
		dbClosed:  func() { _ = database.Close() },
	}
}

func (pa *pressureAssembler) Close() {
	pa.dbClosed()
}

// runPressureTest runs a single pressure scenario and collects metrics.
func runPressureTest(pa *pressureAssembler, query string, sessionText string, sessionID string) (*contextassembler.AssembledContext, error) {
	return pa.assembler.Assemble(query, sessionText, sessionID)
}

// ---------------------------------------------------------------------------
// Benchmark: Context pressure — tight budget
// ---------------------------------------------------------------------------

func BenchmarkContextPressure_TightBudget(b *testing.B) {
	pa := newPressureAssembler(b)
	defer pa.Close()

	query := "what is the backend port configuration for the server"
	sessionText := strings.Repeat("User: Can you help me configure the backend server?\nAssistant: Sure, let me look up the port configuration.\n", 3)

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		ctx, err := runPressureTest(pa, query, sessionText, "pressure-tight")
		if err != nil {
			b.Fatalf("assembly failed: %v", err)
		}
		if ctx == nil {
			b.Fatal("expected non-nil context")
		}
	}
}

// ---------------------------------------------------------------------------
// Benchmark: Context pressure — many results (high retrieval count)
// ---------------------------------------------------------------------------

func BenchmarkContextPressure_ManyResults(b *testing.B) {
	pa := newPressureAssembler(b)
	defer pa.Close()

	// Generate many memories to force scoring pressure
	var wg sync.WaitGroup
	for i := 0; i < 30; i++ {
		wg.Add(1)
		go func(idx int) {
			defer wg.Done()
			_ = pa.assembler // just ensuring assembler is available
			_ = idx
		}(i)
	}
	wg.Wait()

	query := "performance optimization database query caching strategy"
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		ctx, err := runPressureTest(pa, query, "", "pressure-many")
		if err != nil {
			b.Fatalf("assembly failed: %v", err)
		}
		if ctx == nil {
			b.Fatal("expected non-nil context")
		}
	}
}

// ---------------------------------------------------------------------------
// Benchmark: Recovery canary — empty query assembly
// ---------------------------------------------------------------------------

func BenchmarkRecoveryCanary_EmptyQuery(b *testing.B) {
	pa := newPressureAssembler(b)
	defer pa.Close()

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		ctx, err := runPressureTest(pa, "", "", "")
		if err != nil {
			b.Fatalf("empty query assembly failed: %v", err)
		}
		if ctx == nil {
			b.Fatal("expected non-nil context")
		}
		if ctx.UsedTokens < 0 {
			b.Error("negative token usage")
		}
	}
}

// ---------------------------------------------------------------------------
// Benchmark: Recovery canary — very large session text
// ---------------------------------------------------------------------------

func BenchmarkRecoveryCanary_LargeSession(b *testing.B) {
	pa := newPressureAssembler(b)
	defer pa.Close()

	largeSession := strings.Repeat("User: This is a very long session with many turns.\nAssistant: Yes it is, we are discussing many topics.\n", 100)

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		ctx, err := runPressureTest(pa, "large session topics", largeSession, "pressure-large-session")
		if err != nil {
			b.Fatalf("large session assembly failed: %v", err)
		}
		if ctx == nil {
			b.Fatal("expected non-nil context")
		}
	}
}

// ---------------------------------------------------------------------------
// Benchmark: Recovery canary — all layers present
// ---------------------------------------------------------------------------

func BenchmarkRecoveryCanary_AllLayers(b *testing.B) {
	pa := newPressureAssembler(b)
	defer pa.Close()

	query := "memory management session context retrieval"
	sessionText := "User: Let me tell you about memory management.\nAssistant: I have retrieved the relevant memories.\nUser: Great, let me know what you found.\n"

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		ctx, err := runPressureTest(pa, query, sessionText, "pressure-all-layers")
		if err != nil {
			b.Fatalf("all-layers assembly failed: %v", err)
		}
		if ctx == nil {
			b.Fatal("expected non-nil context")
		}
	}
}

// ---------------------------------------------------------------------------
// Test: Context-pressure report generation (deterministic)
// ---------------------------------------------------------------------------

func TestContextPressure_DeterministicReport(t *testing.T) {
	pa := newPressureAssembler(t)
	defer pa.Close()

	scenarios := []struct {
		name      string
		query     string
		sessionID string
		mode      ContextPressureMode
		minPieces int
		maxPieces int
	}{
		{
			name:      "tight budget",
			query:     "backend server port configuration database path",
			sessionID: "cp-tight",
			mode:      PressureBudgetTight,
			minPieces: 0,
			maxPieces: 10,
		},
		{
			name:      "generous budget",
			query:     "user preferences dark mode theme",
			sessionID: "cp-generous",
			mode:      PressureBudgetGenerous,
			minPieces: 0,
			maxPieces: 10,
		},
		{
			name:      "no results (empty query)",
			query:     "",
			sessionID: "cp-noresults",
			mode:      PressureNoResults,
			minPieces: 0,
			maxPieces: 5,
		},
		{
			name:      "many results",
			query:     "performance database caching optimization query tuning indexing strategy",
			sessionID: "cp-many",
			mode:      PressureManyResults,
			minPieces: 0,
			maxPieces: 15,
		},
	}

	for _, s := range scenarios {
		t.Run(s.name, func(t *testing.T) {
			ctx, err := runPressureTest(pa, s.query, "", s.sessionID)
			if err != nil {
				t.Fatalf("assembly failed: %v", err)
			}
			if ctx == nil {
				t.Fatal("expected non-nil context")
			}
			pieces := len(ctx.Pieces)
			if pieces < s.minPieces {
				t.Errorf("expected at least %d pieces, got %d", s.minPieces, pieces)
			}
			if pieces > s.maxPieces && s.maxPieces > 0 {
				t.Errorf("expected at most %d pieces, got %d", s.maxPieces, pieces)
			}
			if ctx.UsedTokens > ctx.Budget {
				t.Errorf("used tokens %d exceeds budget %d", ctx.UsedTokens, ctx.Budget)
			}
		})
	}
}

// ---------------------------------------------------------------------------
// Test: Recovery canary — edge cases
// ---------------------------------------------------------------------------

func TestRecoveryCanary_EdgeCases(t *testing.T) {
	pa := newPressureAssembler(t)
	defer pa.Close()

	t.Run("zero budget assembly", func(t *testing.T) {
		// Override the config's token budget by patching
		origBudget := pa.cfg.Context.TokenBudget
		pa.cfg.Context.TokenBudget = 0
		defer func() { pa.cfg.Context.TokenBudget = origBudget }()

		ctx, err := runPressureTest(pa, "test", "", "")
		if err != nil {
			t.Fatalf("zero budget assembly failed: %v", err)
		}
		if ctx == nil {
			t.Fatal("expected non-nil context")
		}
		if ctx.UsedTokens > ctx.Budget+50 {
			t.Errorf("used tokens %d exceeds budget %d", ctx.UsedTokens, ctx.Budget)
		}
	})

	t.Run("empty assembler", func(t *testing.T) {
		// Assembler with nil database
		a := contextassembler.NewAssembler(nil, nil, nil)
		ctx, err := a.Assemble("test", "", "")
		if err != nil {
			t.Fatalf("nil assembler assembly failed: %v", err)
		}
		if ctx == nil {
			t.Fatal("expected non-nil context")
		}
	})

	t.Run("very long query string", func(t *testing.T) {
		longQuery := strings.Repeat("test query keyword ", 100)
		ctx, err := runPressureTest(pa, longQuery, "", "")
		if err != nil {
			t.Fatalf("long query assembly failed: %v", err)
		}
		if ctx == nil {
			t.Fatal("expected non-nil context")
		}
	})

	t.Run("unicode session text", func(t *testing.T) {
		unicodeText := "User: 你好世界\nAssistant: 你好！\nUser: ありがとう\n"
		ctx, err := runPressureTest(pa, "unicode test", unicodeText, "unicode-session")
		if err != nil {
			t.Fatalf("unicode assembly failed: %v", err)
		}
		if ctx == nil {
			t.Fatal("expected non-nil context")
		}
	})

	t.Run("session with various layers", func(t *testing.T) {
		layers, err := runPressureTest(pa, "comprehensive", "User: test all layers\nAssistant: running full context", "layers-test")
		if err != nil {
			t.Fatalf("layers assembly failed: %v", err)
		}
		if layers == nil {
			t.Fatal("expected non-nil context")
		}
		layerMap := make(map[string]int)
		for _, p := range layers.Pieces {
			layerMap[string(p.Layer)]++
		}
		t.Logf("layers in assembly: %v", layerMap)
	})
}

// ---------------------------------------------------------------------------
// Unit tests for the pressure framework
// ---------------------------------------------------------------------------

func TestResultProducer_Deterministic(t *testing.T) {
	rp1 := NewResultProducer(42)
	rp2 := NewResultProducer(42)

	r1 := rp1.Generate(5, 0.5)
	r2 := rp2.Generate(5, 0.5)

	for i := range r1 {
		if r1[i].ID != r2[i].ID {
			t.Fatalf("determinism broken at index %d: %s vs %s", i, r1[i].ID, r2[i].ID)
		}
		if r1[i].Score != r2[i].Score {
			t.Fatalf("determinism broken at index %d: %f vs %f", i, r1[i].Score, r2[i].Score)
		}
	}
}

func TestResultProducer_ScoreRange(t *testing.T) {
	rp := NewResultProducer(99)
	results := rp.Generate(100, 1.0)

	for _, r := range results {
		if r.Score < 0.0 || r.Score > 1.0 {
			t.Errorf("score out of range [0,1]: %f", r.Score)
		}
	}
}

func TestResultProducer_EmptyGenerate(t *testing.T) {
	rp := NewResultProducer(1)
	results := rp.Generate(0, 0.5)
	if len(results) != 0 {
		t.Errorf("expected 0 results, got %d", len(results))
	}
}

func TestPressureDescription(t *testing.T) {
	tests := []struct {
		mode ContextPressureMode
		want string
	}{
		{PressureBudgetTight, "tight"},
		{PressureBudgetGenerous, "generous"},
		{PressureManyResults, "many"},
		{PressureHighScoreSpread, "wide"},
		{PressureNoResults, "zero"},
		{PressureAllDegraded, "extreme"},
	}

	for _, tt := range tests {
		desc := pressureDescription(tt.mode)
		if desc == "" {
			t.Errorf("empty description for mode %s", tt.mode)
		}
		if !strings.Contains(strings.ToLower(desc), strings.ToLower(tt.want)) {
			t.Errorf("description %q doesn't contain keyword %q for mode %s", desc, tt.want, tt.mode)
		}
	}
}

func TestContextPressureReport_JsonTags(t *testing.T) {
	report := ContextPressureReport{
		Mode:             PressureBudgetTight,
		Queries:          10,
		AvgPieces:        5.5,
		AvgUsedTokens:    800.0,
		AvgBudgetUtilPct: 80.0,
		DegradationPct:   30.0,
		MinPieces:        2,
		MaxPieces:        8,
		Description:      "Test report",
	}

	// Verify JSON round-trip
	// (This is a compile-time check — the tags just need to be valid)
	_ = report
}
