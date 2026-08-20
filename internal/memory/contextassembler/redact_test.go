package contextassembler

import (
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/extractor"
)

// TestAssemble_RedactsLegacySecretInWorkingMemory guards #515 on the
// context-assembly transport: a working memory written without write-time
// redaction (simulated via a direct db.SaveMemory call, bypassing
// internal/memory.Prepare) must not carry raw secret content into the
// assembled prompt context.
func TestAssemble_RedactsLegacySecretInWorkingMemory(t *testing.T) {
	cfg := config.Defaults()
	cfg.WorkingMemory.IncludeInContext = true
	cfg.WorkingMemory.MaxItems = 5

	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	secret := "AKIA1234567890ABCDEF"
	futureExpiry := time.Now().UTC().Add(24 * time.Hour)
	if err := database.SaveMemory(&db.Memory{
		ID:        "wm-legacy-secret",
		Content:   "AWS key is " + secret,
		Scope:     "project",
		Tier:      "working",
		ExpiresAt: &futureExpiry,
		Metadata:  map[string]string{},
	}); err != nil {
		t.Fatalf("SaveMemory failed: %v", err)
	}

	a := NewAssembler(database, nil, &cfg.Context)
	a.SetWorkingMemoryConfig(&cfg.WorkingMemory)

	ctx, err := a.Assemble("", "", "")
	if err != nil {
		t.Fatal(err)
	}
	for _, p := range ctx.Pieces {
		if strings.Contains(p.Content, secret) {
			t.Fatalf("assembled context leaked raw secret in layer %q: %s", p.Layer, p.Content)
		}
	}
}

// TestAssemble_RedactsLegacySecretInRetrieval guards #518 on the
// semantic-retrieval transport: a legacy record (written via a direct
// db.SaveMemory call, bypassing internal/memory.Prepare) returned by
// retrieveRelevant() must not carry raw secret content into the assembled
// prompt context. A real embeddings generator is passed (instead of nil) so
// the nil-embeddings guard is bypassed and the retrieval layer is
// exercised; the query vector is deterministic because it is derived from
// the same content used to seed the memory's embedding, so SearchMemories
// returns the seeded record (offline hash-fallback when Ollama is absent).
func TestAssemble_RedactsLegacySecretInRetrieval(t *testing.T) {
	cfg := config.Defaults()
	cfg.Database.Path = filepath.Join(t.TempDir(), "test.db")
	cfg.WorkingMemory.IncludeInContext = true
	cfg.WorkingMemory.MaxItems = 5

	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	secret := "gho_abcdefabcdefabcdefabcdefabcdefabcdef"
	content := "Deployment note: GitHub auth token is " + secret
	embeddings := extractor.NewEmbeddingsGenerator(config.Defaults())
	emb := embeddings.GenerateVector(content)
	if err := database.SaveMemory(&db.Memory{
		ID:              "retr-legacy-secret",
		Content:         content,
		Scope:           "project",
		Embedding:       emb.Vector,
		EmbeddingSource: emb.Source,
		EmbeddingModel:  emb.Model,
	}); err != nil {
		t.Fatalf("SaveMemory failed: %v", err)
	}

	a := NewAssembler(database, embeddings, &cfg.Context)
	a.SetWorkingMemoryConfig(&cfg.WorkingMemory)

	ctx, err := a.Assemble(content, "", "")
	if err != nil {
		t.Fatal(err)
	}
	sawRedactedRetrieval := false
	for _, p := range ctx.Pieces {
		if strings.Contains(p.Content, secret) {
			t.Fatalf("assembled context leaked raw secret in layer %q: %s", p.Layer, p.Content)
		}
		if p.Layer == LayerRetrieval && strings.Contains(p.Content, "[REDACTED_API_KEY]") {
			sawRedactedRetrieval = true
		}
	}
	if !sawRedactedRetrieval {
		t.Fatalf("expected a redacted retrieval piece, got pieces: %+v", ctx.Pieces)
	}
}
