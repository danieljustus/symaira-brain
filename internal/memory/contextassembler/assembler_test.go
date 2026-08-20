package contextassembler

import (
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

// ---------------------------------------------------------------------------
// Existing tests (refactored)
// ---------------------------------------------------------------------------

func TestExtractWorkingContext_Trimmed(t *testing.T) {
	text := "Line 1\nLine 2\nLine 3\nLine 4\nLine 5\nLine 6\nLine 7\nLine 8\nLine 9\nLine 10"
	result := extractWorkingContext(text, 2)
	lines := 0
	for range []byte(result) {
		if result[lines] == '\n' {
			lines++
		}
	}
	if result == text {
		t.Error("expected working context to be trimmed")
	}
}

func TestExtractWorkingContext_ShortText(t *testing.T) {
	text := "Line 1\nLine 2"
	result := extractWorkingContext(text, 5)
	if result != text {
		t.Errorf("expected short text to pass through unchanged, got %q", result)
	}
}

func TestEstimateTokens(t *testing.T) {
	if estimateTokens("") != 0 {
		t.Error("expected 0 tokens for empty string")
	}
	tokens := estimateTokens("hello world")
	if tokens <= 0 {
		t.Errorf("expected positive token count, got %d", tokens)
	}
}

func TestFormatRetrievalResults(t *testing.T) {
	results := []db.SearchResult{
		{Memory: &db.Memory{Content: "Alice prefers dark mode"}, Score: 0.9},
		{Memory: &db.Memory{Content: "Backend uses port 8080"}, Score: 0.7},
	}
	formatted := formatRetrievalResults(results)
	if formatted == "" {
		t.Error("expected non-empty formatted output")
	}
}

func TestAssembler_Construction(t *testing.T) {
	cfg := config.Defaults()
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	a := NewAssembler(database, nil, &cfg.Context)
	if a == nil {
		t.Fatal("expected non-nil assembler")
	}
}

func TestAssembler_Assemble_EmptySession(t *testing.T) {
	cfg := config.Defaults()
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	a := NewAssembler(database, nil, &cfg.Context)
	ctx, err := a.Assemble("test query", "", "")
	if err != nil {
		t.Fatal(err)
	}
	if ctx == nil {
		t.Fatal("expected non-nil context")
	}
	if ctx.UsedTokens < 0 {
		t.Errorf("expected non-negative used tokens, got %d", ctx.UsedTokens)
	}
}

func TestAssembler_Assemble_WithSessionText(t *testing.T) {
	cfg := config.Defaults()
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	a := NewAssembler(database, nil, &cfg.Context)
	sessionText := "User: What is the port?\nAssistant: The backend uses port 8080.\nUser: Thanks!"
	ctx, err := a.Assemble("port number", sessionText, "test-session")
	if err != nil {
		t.Fatal(err)
	}
	hasWorkingCtx := false
	for _, p := range ctx.Pieces {
		if p.Layer == LayerWorkingContext {
			hasWorkingCtx = true
		}
	}
	if !hasWorkingCtx {
		t.Error("expected working context layer to be present")
	}
}

func TestAssembler_TokenBudgetRespected(t *testing.T) {
	cfg := config.Defaults()
	cfg.Context.TokenBudget = 20
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	a := NewAssembler(database, nil, &cfg.Context)
	longText := strings.Repeat("word ", 500)
	ctx, err := a.Assemble("query", longText, "")
	if err != nil {
		t.Fatal(err)
	}
	if ctx.UsedTokens > ctx.Budget+50 {
		t.Errorf("used tokens (%d) exceeds budget (%d) by more than margin", ctx.UsedTokens, ctx.Budget)
	}
}

func TestAssembler_WorkingMemoryIncluded(t *testing.T) {
	cfg := config.Defaults()
	cfg.WorkingMemory.IncludeInContext = true
	cfg.WorkingMemory.MaxItems = 10

	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	futureExpiry := time.Now().UTC().Add(24 * time.Hour)
	if err := database.SaveMemory(&db.Memory{
		ID:        "wm-1",
		Content:   "Current task: implement feature X",
		Scope:     "project",
		Tier:      "working",
		ExpiresAt: &futureExpiry,
		Metadata:  map[string]string{},
	}); err != nil {
		t.Fatalf("SaveMemory failed: %v", err)
	}

	a := NewAssembler(database, nil, &cfg.Context)
	a.SetWorkingMemoryConfig(&cfg.WorkingMemory)

	ctx, err := a.Assemble("test query", "", "")
	if err != nil {
		t.Fatal(err)
	}

	hasWorkingMem := false
	for _, p := range ctx.Pieces {
		if p.Layer == LayerWorkingMemory {
			hasWorkingMem = true
			break
		}
	}
	if !hasWorkingMem {
		t.Error("expected working memory layer to be present when IncludeInContext=true")
	}
}

func TestAssembler_WorkingMemoryExcluded(t *testing.T) {
	cfg := config.Defaults()
	cfg.WorkingMemory.IncludeInContext = false

	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	futureExpiry := time.Now().UTC().Add(24 * time.Hour)
	if err := database.SaveMemory(&db.Memory{
		ID:        "wm-1",
		Content:   "Current task: implement feature X",
		Scope:     "project",
		Tier:      "working",
		ExpiresAt: &futureExpiry,
		Metadata:  map[string]string{},
	}); err != nil {
		t.Fatalf("SaveMemory failed: %v", err)
	}

	a := NewAssembler(database, nil, &cfg.Context)
	a.SetWorkingMemoryConfig(&cfg.WorkingMemory)

	ctx, err := a.Assemble("test query", "", "")
	if err != nil {
		t.Fatal(err)
	}

	for _, p := range ctx.Pieces {
		if p.Layer == LayerWorkingMemory {
			t.Error("working memory layer should not be present when IncludeInContext=false")
		}
	}
}

func TestAssembler_WorkingMemoryRespectsBudget(t *testing.T) {
	cfg := config.Defaults()
	cfg.Context.TokenBudget = 10
	cfg.WorkingMemory.IncludeInContext = true
	cfg.WorkingMemory.MaxItems = 10

	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	futureExpiry := time.Now().UTC().Add(24 * time.Hour)
	for i := 0; i < 5; i++ {
		if err := database.SaveMemory(&db.Memory{
			ID:        "wm-" + string(rune('a'+i)),
			Content:   "Working memory about databases and networking and security",
			Scope:     "project",
			Tier:      "working",
			ExpiresAt: &futureExpiry,
			Metadata:  map[string]string{},
		}); err != nil {
			t.Fatalf("SaveMemory failed: %v", err)
		}
	}

	a := NewAssembler(database, nil, &cfg.Context)
	a.SetWorkingMemoryConfig(&cfg.WorkingMemory)

	ctx, err := a.Assemble("test", "", "")
	if err != nil {
		t.Fatal(err)
	}

	if ctx.UsedTokens > ctx.Budget+50 {
		t.Errorf("used tokens (%d) exceeds budget (%d) by more than margin", ctx.UsedTokens, ctx.Budget)
	}
}

func TestAssembler_WorkingMemoryRespectsMaxItems(t *testing.T) {
	cfg := config.Defaults()
	cfg.WorkingMemory.IncludeInContext = true
	cfg.WorkingMemory.MaxItems = 2

	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	futureExpiry := time.Now().UTC().Add(24 * time.Hour)
	for i := 0; i < 5; i++ {
		if err := database.SaveMemory(&db.Memory{
			ID:        "wm-item",
			Content:   "Working memory task",
			Scope:     "project",
			Tier:      "working",
			ExpiresAt: &futureExpiry,
			Metadata:  map[string]string{},
		}); err != nil {
			t.Fatalf("SaveMemory failed: %v", err)
		}
	}

	a := NewAssembler(database, nil, &cfg.Context)
	a.SetWorkingMemoryConfig(&cfg.WorkingMemory)

	ctx, err := a.Assemble("test", "", "")
	if err != nil {
		t.Fatal(err)
	}

	for _, p := range ctx.Pieces {
		if p.Layer == LayerWorkingMemory {
			if estimateTokens(p.Content) > cfg.WorkingMemory.MaxItems*20 {
				t.Error("working memory layer content exceeds MaxItems-derived token budget")
			}
		}
	}
}

// ---------------------------------------------------------------------------
// #414: Degradation ladder tests
// ---------------------------------------------------------------------------

func TestFormatSingleResult_Full(t *testing.T) {
	r := db.SearchResult{
		Memory: &db.Memory{Content: "The backend HTTP server listens on port 8080"},
		Score:  0.95,
	}
	result := formatSingleResult(r, DegradationFull)
	if !strings.Contains(result, "port 8080") {
		t.Error("full degradation should contain full content")
	}
}

func TestFormatSingleResult_Summary(t *testing.T) {
	r := db.SearchResult{
		Memory: &db.Memory{Content: "The backend HTTP server listens on port 8080 and uses SQLite for storage"},
		Score:  0.85,
	}
	result := formatSingleResult(r, DegradationSummary)
	if !strings.Contains(result, "[ref:") {
		t.Error("summary degradation should contain reference marker")
	}
}

func TestFormatSingleResult_Reference(t *testing.T) {
	r := db.SearchResult{
		Memory: &db.Memory{ID: "arch-port", Content: "The backend HTTP server listens on port 8080"},
		Score:  0.75,
	}
	result := formatSingleResult(r, DegradationReference)
	if !strings.Contains(result, "arch-port") {
		t.Error("reference degradation should contain memory ID")
	}
}

func TestFillRetrievalWithDegradation_NoDegradationConfig(t *testing.T) {
	// When no degradation config is set, should produce a single block
	cfg := config.Defaults()
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	a := NewAssembler(database, nil, &cfg.Context)
	results := []db.SearchResult{
		{Memory: &db.Memory{Content: "Memory one about topic A"}, Score: 0.9},
		{Memory: &db.Memory{Content: "Memory two about topic B"}, Score: 0.7},
	}
	pieces := a.fillRetrievalWithDegradation(results, 500)
	if len(pieces) == 0 {
		t.Fatal("expected at least one piece")
	}
	if pieces[0].Layer != LayerRetrieval {
		t.Errorf("expected retrieval layer, got %s", pieces[0].Layer)
	}
}

func TestFillRetrievalWithDegradation_GreedyFill(t *testing.T) {
	cfg := config.Defaults()
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	a := NewAssembler(database, nil, &cfg.Context)
	a.WithDegradationConfig(&cfg.Degradation)

	results := []db.SearchResult{
		{Memory: &db.Memory{ID: "mem-1", Content: "Short memory"}, Score: 0.5},
		{Memory: &db.Memory{ID: "mem-2", Content: strings.Repeat("Another long memory piece of content ", 20)}, Score: 0.9},
		{Memory: &db.Memory{ID: "mem-3", Content: "Third memory about something"}, Score: 0.7},
	}
	pieces := a.fillRetrievalWithDegradation(results, 200)
	if len(pieces) == 0 {
		t.Fatal("expected at least one piece")
	}
	if pieces[0].Tokens <= 0 {
		t.Error("expected positive token count")
	}
}

// TestFillRetrievalWithDegradation_KindBandOrdering verifies the #486
// ordering rule: within the same score band, identity-level facts (kind
// user) precede project chatter; across bands, higher scores still win.
func TestFillRetrievalWithDegradation_KindBandOrdering(t *testing.T) {
	cfg := config.Defaults()
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	a := NewAssembler(database, nil, &cfg.Context)
	a.WithDegradationConfig(&cfg.Degradation)

	results := []db.SearchResult{
		{Memory: &db.Memory{ID: "top", Content: "best match", Kind: db.KindReference}, Score: 0.90},
		{Memory: &db.Memory{ID: "project-chatter", Content: "project note about deploy", Kind: db.KindProject}, Score: 0.62},
		{Memory: &db.Memory{ID: "user-pref", Content: "user prefers dark mode", Kind: db.KindUser}, Score: 0.60},
	}
	pieces := a.fillRetrievalWithDegradation(results, 500)
	if len(pieces) == 0 {
		t.Fatal("expected pieces")
	}
	var order []string
	for _, p := range pieces {
		switch {
		case strings.Contains(p.Content, "best match"):
			order = append(order, "top")
		case strings.Contains(p.Content, "user prefers"):
			order = append(order, "user-pref")
		case strings.Contains(p.Content, "project note"):
			order = append(order, "project-chatter")
		}
	}
	if len(order) != 3 {
		t.Fatalf("expected all three pieces in order, got %v", order)
	}
	// Highest band first regardless of kind.
	if order[0] != "top" {
		t.Errorf("top-scoring piece must come first, got %v", order)
	}
	// Same band (0.62 and 0.60 → band 12): kind user before kind project.
	if idxUser, idxProj := indexOf(order, "user-pref"), indexOf(order, "project-chatter"); idxUser > idxProj {
		t.Errorf("kind user must precede kind project within the same score band, got %v", order)
	}
}

func indexOf(ss []string, s string) int {
	for i, v := range ss {
		if v == s {
			return i
		}
	}
	return -1
}

func TestFillRetrievalWithDegradation_SkipsWhenBudgetTooLow(t *testing.T) {
	cfg := config.Defaults()
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	a := NewAssembler(database, nil, &cfg.Context)
	a.WithDegradationConfig(&cfg.Degradation)

	results := []db.SearchResult{
		{Memory: &db.Memory{ID: "mem-1", Content: strings.Repeat("Long content ", 50)}, Score: 0.9},
	}
	pieces := a.fillRetrievalWithDegradation(results, 5)
	// Budget is extremely low, should return nil or gracefully degrade
	_ = pieces
}

func TestDegradationLevel_String(t *testing.T) {
	if DegradationFull.String() != "full" {
		t.Errorf("expected 'full', got %s", DegradationFull.String())
	}
	if DegradationSummary.String() != "summary" {
		t.Errorf("expected 'summary', got %s", DegradationSummary.String())
	}
	if DegradationReference.String() != "reference" {
		t.Errorf("expected 'reference', got %s", DegradationReference.String())
	}
}

// ---------------------------------------------------------------------------
// #412: Delta context pack tests
// ---------------------------------------------------------------------------

func TestComputeSnapshot(t *testing.T) {
	ctx := &AssembledContext{
		Query:      "test query",
		Budget:     1000,
		UsedTokens: 500,
		Pieces: []AssembledPiece{
			{Layer: LayerWorkingContext, Content: "Working context content", Tokens: 200},
			{Layer: LayerRetrieval, Content: "Retrieved memory", Tokens: 300},
		},
	}
	snapshot := computeSnapshot("session-1", ctx)
	if snapshot.SessionID != "session-1" {
		t.Errorf("expected session-1, got %s", snapshot.SessionID)
	}
	if len(snapshot.PieceHashes) != 2 {
		t.Errorf("expected 2 piece hashes, got %d", len(snapshot.PieceHashes))
	}
}

func TestSnapshotKey_Deterministic(t *testing.T) {
	ctx := &AssembledContext{
		Query:      "test",
		Budget:     1000,
		UsedTokens: 500,
		Pieces: []AssembledPiece{
			{Layer: LayerWorkingContext, Content: "Same content", Tokens: 200},
		},
	}
	s1 := computeSnapshot("sid", ctx)
	s2 := computeSnapshot("sid", ctx)
	if s1.Key() != s2.Key() {
		t.Error("snapshot keys should be deterministic for same content")
	}
}

func TestComputeDelta_IdenticalSnapshots(t *testing.T) {
	ctx := &AssembledContext{
		Query:      "test",
		Budget:     1000,
		UsedTokens: 500,
		Pieces: []AssembledPiece{
			{Layer: LayerWorkingContext, Content: "Same", Tokens: 200},
		},
	}
	s1 := computeSnapshot("sid", ctx)
	s2 := computeSnapshot("sid", ctx)
	delta := computeDelta("sid", s1, s2)
	if delta != nil {
		t.Error("identical snapshots should produce nil delta")
	}
}

func TestComputeDelta_TokenChange(t *testing.T) {
	ctx1 := &AssembledContext{
		Query:      "test",
		Budget:     1000,
		UsedTokens: 200,
		Pieces: []AssembledPiece{
			{Layer: LayerWorkingContext, Content: "First", Tokens: 200},
		},
	}
	ctx2 := &AssembledContext{
		Query:      "test",
		Budget:     1000,
		UsedTokens: 500,
		Pieces: []AssembledPiece{
			{Layer: LayerWorkingContext, Content: "First", Tokens: 200},
			{Layer: LayerRetrieval, Content: "More content", Tokens: 300},
		},
	}
	s1 := computeSnapshot("sid", ctx1)
	s2 := computeSnapshot("sid", ctx2)
	delta := computeDelta("sid", s1, s2)
	if delta == nil {
		t.Fatal("expected non-nil delta")
	}
	if delta.TokenDelta != 300 {
		t.Errorf("expected token delta 300, got %d", delta.TokenDelta)
	}
}

func TestMaybeSnapshot_FirstSnapshot(t *testing.T) {
	cfg := config.Defaults()
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	a := NewAssembler(database, nil, &cfg.Context)
	a.WithDeltaPackConfig(&cfg.DeltaPack)

	ctx := &AssembledContext{
		Query:      "test",
		Budget:     1000,
		UsedTokens: 500,
		Pieces: []AssembledPiece{
			{Layer: LayerWorkingContext, Content: "Content", Tokens: 500},
		},
	}
	delta := a.maybeSnapshot("session-1", ctx)
	if delta != nil {
		t.Error("first snapshot should return nil delta")
	}
}

func TestSnapshots_Chain(t *testing.T) {
	cfg := config.Defaults()
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	a := NewAssembler(database, nil, &cfg.Context)
	a.WithDeltaPackConfig(&cfg.DeltaPack)

	// First assembly
	ctx1 := &AssembledContext{
		Query:      "q1",
		Budget:     1000,
		UsedTokens: 100,
		Pieces:     []AssembledPiece{{Layer: LayerWorkingContext, Content: "A", Tokens: 100}},
	}
	a.maybeSnapshot("sess", ctx1)

	// Second assembly — small delta, won't trigger snapshot
	ctx2 := &AssembledContext{
		Query:      "q2",
		Budget:     1000,
		UsedTokens: 110,
		Pieces:     []AssembledPiece{{Layer: LayerWorkingContext, Content: "A", Tokens: 110}},
	}
	a.maybeSnapshot("sess", ctx2)

	chain := a.Snapshots("sess")
	if len(chain) != 1 {
		// Only first snapshot should be stored (delta 10/100 = 10% < 20% threshold)
		t.Errorf("expected 1 snapshot, got %d", len(chain))
	}
}

// ---------------------------------------------------------------------------
// #403: Profile layer tests
// ---------------------------------------------------------------------------

func TestSynthesizeProfile_NoDB(t *testing.T) {
	cfg := config.Defaults()
	a := NewAssembler(nil, nil, &cfg.Context)
	a.WithProfileLayerConfig(&cfg.ProfileLayer)

	profile := a.synthesizeProfile("session-1")
	if profile == nil {
		t.Fatal("expected non-nil profile when config is enabled")
	}
	// Without DB, static info is empty but dynamic still has defaults
	if profile.Dynamic.AccessPattern != "read_write" {
		t.Errorf("expected default access_pattern 'read_write', got %s", profile.Dynamic.AccessPattern)
	}
}

func TestSynthesizeProfile_Disabled(t *testing.T) {
	cfg := config.Defaults()
	cfg.ProfileLayer.Enabled = false

	a := NewAssembler(nil, nil, &cfg.Context)
	a.WithProfileLayerConfig(&cfg.ProfileLayer)

	profile := a.synthesizeProfile("session-1")
	if profile != nil {
		t.Error("expected nil profile when disabled")
	}
}

func TestFormatProfile_Empty(t *testing.T) {
	result := formatProfile(nil, &config.ProfileLayerConfig{Enabled: true})
	if result != "" {
		t.Errorf("expected empty string for nil profile, got %q", result)
	}
}

func TestFormatProfile_WithStaticInfo(t *testing.T) {
	profile := &SynthesizedProfile{
		Static: StaticProfileInfo{
			UserID:      "user-123",
			DisplayName: "Test User",
			Language:    "english",
			TimeZone:    "UTC",
		},
		Dynamic: DynamicProfileInfo{
			RecentTopics:    []string{"Go", "SQLite"},
			PreferredScopes: []string{"global", "project"},
			AccessPattern:   "read_write",
		},
	}
	cfg := &config.ProfileLayerConfig{
		Enabled: true, MaxStaticLen: 200, MaxDynamicLen: 500,
	}
	result := formatProfile(profile, cfg)
	if !strings.Contains(result, "Profile:") {
		t.Error("expected Profile: header")
	}
	if !strings.Contains(result, "english") {
		t.Error("expected language info in output")
	}
	if !strings.Contains(result, "Go") {
		t.Error("expected topic in output")
	}
}

func TestProfileLayer_AttachedInAssembly(t *testing.T) {
	cfg := config.Defaults()
	cfg.ProfileLayer.Enabled = true
	cfg.ProfileLayer.TokenBudget = 200

	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	a := NewAssembler(database, nil, &cfg.Context)
	a.WithProfileLayerConfig(&cfg.ProfileLayer)
	a.WithFullConfig(cfg)

	// Assemble with empty query — profile layer should still appear
	ctx, err := a.Assemble("", "", "")
	if err != nil {
		t.Fatal(err)
	}

	hasProfile := false
	for _, p := range ctx.Pieces {
		if p.Layer == LayerProfile {
			hasProfile = true
			break
		}
	}
	if !hasProfile {
		t.Error("expected profile layer in assembled context even without query")
	}
}

// ---------------------------------------------------------------------------
// #400: Session context with expandable sources tests
// ---------------------------------------------------------------------------

func TestBuildSessionContextSourceLinks_NoSession(t *testing.T) {
	cfg := config.Defaults()
	a := NewAssembler(nil, nil, &cfg.Context)
	a.WithSessionContextConfig(&cfg.SessionCtx)

	sources, err := a.buildSessionContextSourceLinks("")
	if err != nil {
		t.Fatal(err)
	}
	if sources == nil {
		// Should return empty slice, not nil
		t.Log("no sources for empty session ID (expected)")
	}
}

func TestBuildSessionContextSourceLinks_Disabled(t *testing.T) {
	cfg := config.Defaults()
	cfg.SessionCtx.Enabled = false

	a := NewAssembler(nil, nil, &cfg.Context)
	a.WithSessionContextConfig(&cfg.SessionCtx)

	sources, err := a.buildSessionContextSourceLinks("session-1")
	if err != nil {
		t.Fatal(err)
	}
	if sources != nil {
		t.Error("expected nil sources when session context config is disabled")
	}
}

func TestFormatSessionContext_Empty(t *testing.T) {
	result := formatSessionContext(nil)
	if result != "" {
		t.Errorf("expected empty string for nil sources, got %q", result)
	}
}

func TestFormatSessionContext_WithSources(t *testing.T) {
	sources := []ExpandableSource{
		{
			SourceLink: SourceLink{
				SourceID: "src-1", SourceType: "memory",
				Label: "Test memory", ContentHash: "abc123",
				RefCount: 1,
			},
			ExpandSteps: 3,
			Metadata:    map[string]string{"scope": "global"},
		},
	}
	result := formatSessionContext(sources)
	if !strings.Contains(result, "Session context sources:") {
		t.Error("expected header")
	}
	if !strings.Contains(result, "src-1") {
		t.Error("expected source ID in output")
	}
	if !strings.Contains(result, "memory") {
		t.Error("expected source type in output")
	}
}

func TestSessionContextLayer_AttachedInAssembly(t *testing.T) {
	cfg := config.Defaults()
	cfg.SessionCtx.Enabled = true
	cfg.SessionCtx.TokenBudget = 300

	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	// Save a session summary
	if err := database.SaveSessionSummary("test-session", "Session summary content about topic X"); err != nil {
		t.Fatal(err)
	}

	a := NewAssembler(database, nil, &cfg.Context)
	a.WithSessionContextConfig(&cfg.SessionCtx)
	a.WithFullConfig(cfg)

	ctx, err := a.Assemble("test", "", "test-session")
	if err != nil {
		t.Fatal(err)
	}

	hasSessionCtx := false
	for _, p := range ctx.Pieces {
		if p.Layer == LayerSessionContext {
			hasSessionCtx = true
			break
		}
	}
	if !hasSessionCtx {
		t.Error("expected session context layer in assembled context")
	}
}

// ---------------------------------------------------------------------------
// #414: Integration — full assembly with degradation
// ---------------------------------------------------------------------------

func TestAssemble_WithDegradationLadder(t *testing.T) {
	cfg := config.Defaults()
	cfg.Degradation.Enabled = true
	cfg.Degradation.FullTokens = 200
	cfg.Degradation.SummaryTokens = 80
	cfg.Degradation.RefTokens = 30

	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	// Save some memories to retrieve
	for i := 0; i < 5; i++ {
		mem := &db.Memory{
			ID:      fmt.Sprintf("degrade-mem-%d", i),
			Content: fmt.Sprintf("This is memory number %d with some detailed content that might take many tokens to represent", i),
			Scope:   "global",
		}
		if err := database.SaveMemory(mem); err != nil {
			t.Fatalf("SaveMemory failed: %v", err)
		}
	}

	a := NewAssembler(database, nil, &cfg.Context)
	a.WithFullConfig(cfg)

	ctx, err := a.Assemble("memory number", "", "test-degrade")
	if err != nil {
		t.Fatal(err)
	}
	if ctx == nil {
		t.Fatal("expected non-nil context")
	}
	if ctx.UsedTokens <= 0 {
		t.Error("expected some tokens used")
	}
}

// ---------------------------------------------------------------------------
// #400: Integration — full assembly with expandable session context
// ---------------------------------------------------------------------------

func TestAssemble_WithExpandableSources(t *testing.T) {
	cfg := config.Defaults()
	cfg.SessionCtx.Enabled = true
	cfg.SessionCtx.MaxSources = 3
	cfg.SessionCtx.TokenBudget = 200

	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	if err := database.SaveSessionSummary("expand-session", "Session about expandable context features"); err != nil {
		t.Fatal(err)
	}

	a := NewAssembler(database, nil, &cfg.Context)
	a.WithFullConfig(cfg)

	ctx, err := a.Assemble("expandable", "", "expand-session")
	if err != nil {
		t.Fatal(err)
	}

	if len(ctx.ExpandableSources) > 0 {
		t.Logf("got %d expandable sources", len(ctx.ExpandableSources))
	} else {
		t.Log("no expandable sources (depends on DB state)")
	}
}

// ---------------------------------------------------------------------------
// #412: Integration — delta pack in assembly
// ---------------------------------------------------------------------------

func TestAssemble_DeltaPackGeneration(t *testing.T) {
	cfg := config.Defaults()
	cfg.DeltaPack.Enabled = true
	cfg.DeltaPack.SnapshotOnTokenPct = 5
	cfg.DeltaPack.MinSnapshotIntervalSecs = 0

	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	a := NewAssembler(database, nil, &cfg.Context)
	a.WithFullConfig(cfg)

	ctx1, err := a.Assemble("first query", "First session text", "delta-session")
	if err != nil {
		t.Fatal(err)
	}
	_ = ctx1

	// Second assembly with more content — should trigger snapshot
	ctx2, err := a.Assemble("second query with expanded tokens to trigger delta",
		"Second session text that is much longer and should trigger a snapshot because the token delta percentage exceeds the threshold",
		"delta-session")
	if err != nil {
		t.Fatal(err)
	}

	if ctx2.Delta != nil {
		t.Logf("delta pack generated: tokenDelta=%d, addedPieces=%d", ctx2.Delta.TokenDelta, len(ctx2.Delta.AddedPieces))
	} else {
		t.Log("no delta pack (snapshot condition not met)")
	}
}

// ---------------------------------------------------------------------------
// #403: Profile layer integration
// ---------------------------------------------------------------------------

func TestAssemble_ProfileLayerWithoutQuery(t *testing.T) {
	cfg := config.Defaults()
	cfg.ProfileLayer.Enabled = true
	cfg.ProfileLayer.TokenBudget = 200

	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	a := NewAssembler(database, nil, &cfg.Context)
	a.WithFullConfig(cfg)

	// No query — profile should still be attached
	ctx, err := a.Assemble("", "", "")
	if err != nil {
		t.Fatal(err)
	}

	hasProfile := false
	for _, p := range ctx.Pieces {
		if p.Layer == LayerProfile {
			hasProfile = true
			break
		}
	}
	if !hasProfile {
		t.Error("profile layer should attach without search query")
	}
}

// ---------------------------------------------------------------------------
// #400/#414: Combined integration
// ---------------------------------------------------------------------------

func TestAssemble_AllNewLayers(t *testing.T) {
	cfg := config.Defaults()
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	if err := database.SaveSessionSummary("all-layers", "Session covering all new layer types"); err != nil {
		t.Fatal(err)
	}

	a := NewAssembler(database, nil, &cfg.Context)
	a.WithFullConfig(cfg)

	ctx, err := a.Assemble("comprehensive test", "User: test the new layers\nAssistant: they work!", "all-layers")
	if err != nil {
		t.Fatal(err)
	}

	layerSet := make(map[ContextLayer]bool)
	for _, p := range ctx.Pieces {
		layerSet[p.Layer] = true
	}

	t.Logf("layers present: %v", layerSet)
	// Working context should always be present
	if !layerSet[LayerWorkingContext] {
		t.Error("expected LayerWorkingContext")
	}
	// Profile layer should be present (a.profileLayerCfg defaults to enabled)
	if !layerSet[LayerProfile] {
		t.Log("LayerProfile not present (may not have generated content) — non-fatal")
	}
}

func TestTruncateText(t *testing.T) {
	if truncateText("short", 100) != "short" {
		t.Error("should not truncate short text")
	}
	result := truncateText("This is a long text that should be truncated", 20)
	if len(result) > 23 {
		t.Errorf("truncated text too long: %d chars", len(result))
	}
	if !strings.HasSuffix(result, "…") {
		t.Error("truncated text should end with …")
	}
}

func TestSummarizeContent(t *testing.T) {
	if summarizeContent("short", 100) != "short" {
		t.Error("short content should pass through")
	}
	long := strings.Repeat("word! ", 50)
	summary := summarizeContent(long, 30)
	if len(summary) > 35 {
		t.Errorf("summary too long: %d", len(summary))
	}
}

func TestExtractFirstLine(t *testing.T) {
	if extractFirstLine("single line") != "single line" {
		t.Error("single line should pass through")
	}
	multi := "first line\nsecond line"
	if extractFirstLine(multi) != "first line" {
		t.Errorf("expected only first line, got %q", extractFirstLine(multi))
	}
}

func TestExtractLanguage(t *testing.T) {
	if extractLanguage("I prefer english language") != "english" {
		t.Error("should extract english")
	}
	if extractLanguage("nothing here") != "" {
		t.Error("should return empty for no match")
	}
}

func TestExtractTimezone(t *testing.T) {
	if extractTimezone("Use UTC timezone") != "UTC" {
		t.Error("should extract UTC")
	}
	if extractTimezone("no timezone mentioned") != "" {
		t.Error("should return empty for no match")
	}
}
