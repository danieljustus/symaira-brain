package contextassembler

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"math"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/extractor"
	"github.com/danieljustus/symaira-brain/internal/memory/memory"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
	"github.com/danieljustus/symaira-brain/internal/memory/summarizer"
)

// ---------------------------------------------------------------------------
// Layers
// ---------------------------------------------------------------------------

type ContextLayer string

const (
	LayerWorkingContext ContextLayer = "working_context"
	LayerWorkingMemory  ContextLayer = "working_memory"
	LayerSummary        ContextLayer = "summary"
	LayerRetrieval      ContextLayer = "retrieval"
	LayerProfile        ContextLayer = "profile"
	LayerSessionContext ContextLayer = "session_context"
)

// ---------------------------------------------------------------------------
// Degradation ladder (#414)
// ---------------------------------------------------------------------------

// DegradationLevel represents how much a retrieval piece has been degraded to
// fit inside a token budget.
type DegradationLevel int

const (
	DegradationFull      DegradationLevel = iota // original content
	DegradationSummary                           // summarised version
	DegradationReference                         // single-line reference only
)

func (d DegradationLevel) String() string {
	switch d {
	case DegradationFull:
		return "full"
	case DegradationSummary:
		return "summary"
	case DegradationReference:
		return "reference"
	default:
		return "unknown"
	}
}

// DegradationPiece pairs a retrieval result with its current degradation level.
type DegradationPiece struct {
	Result db.SearchResult
	Level  DegradationLevel
	Tokens int
}

// ---------------------------------------------------------------------------
// Delta context packs (#412)
// ---------------------------------------------------------------------------

// ContentHashSnapshot is a content-hash based snapshot of an assembled context.
// It is used to compute delta packs between successive assemblies.
type ContentHashSnapshot struct {
	SessionID    string            `json:"session_id"`
	TakenAt      time.Time         `json:"taken_at"`
	UsedTokens   int               `json:"used_tokens"`
	PieceHashes  []string          `json:"piece_hashes"` // sha256 of each piece's content
	LayerOrder   []ContextLayer    `json:"layer_order"`
	SourceHashes map[string]string `json:"source_hashes,omitempty"` // source_id → content_hash
}

// DeltaPack describes what changed between two ContentHashSnapshots.
type DeltaPack struct {
	SessionID     string           `json:"session_id"`
	FromSnapshot  string           `json:"from_snapshot"` // hex digest of prior snapshot
	ToSnapshot    string           `json:"to_snapshot"`   // hex digest of this snapshot
	AddedPieces   []AssembledPiece `json:"added_pieces,omitempty"`
	RemovedLayers []ContextLayer   `json:"removed_layers,omitempty"`
	TokenDelta    int              `json:"token_delta"`
}

// ---------------------------------------------------------------------------
// Profile layer (#403)
// ---------------------------------------------------------------------------

// StaticProfileInfo holds persistent profile attributes (config-derived).
type StaticProfileInfo struct {
	UserID      string `json:"user_id,omitempty"`
	DisplayName string `json:"display_name,omitempty"`
	Role        string `json:"role,omitempty"`
	Language    string `json:"language,omitempty"`
	TimeZone    string `json:"timezone,omitempty"`
}

// DynamicProfileInfo holds session-derived profile attributes.
type DynamicProfileInfo struct {
	SessionCount    int      `json:"session_count,omitempty"`
	RecentTopics    []string `json:"recent_topics,omitempty"`
	PreferredScopes []string `json:"preferred_scopes,omitempty"`
	AccessPattern   string   `json:"access_pattern,omitempty"` // e.g. "read_write", "read_only"
}

// SynthesizedProfile combines static and dynamic profile info into a single
// profile layer that can be attached to assembled context without a search query.
type SynthesizedProfile struct {
	Static  StaticProfileInfo  `json:"static"`
	Dynamic DynamicProfileInfo `json:"dynamic"`
}

// ---------------------------------------------------------------------------
// Expandable session context (#400)
// ---------------------------------------------------------------------------

// SourceLink identifies an expandable source for session context.
type SourceLink struct {
	SourceID    string `json:"source_id"`
	SourceType  string `json:"source_type"` // e.g. "memory", "session", "document", "entity"
	Label       string `json:"label,omitempty"`
	ContentHash string `json:"content_hash,omitempty"`
	RefCount    int    `json:"ref_count,omitempty"`
}

// ExpandableSource is a source that can be expanded (fetched on demand) during
// context assembly.
type ExpandableSource struct {
	SourceLink
	ExpandSteps int               `json:"expand_steps"` // how many expansion steps are available
	Metadata    map[string]string `json:"metadata,omitempty"`
}

// SessionContextLayer holds source-linked, expandable session context.
type SessionContextLayer struct {
	Sources              []ExpandableSource `json:"sources"`
	HasExpandableContent bool               `json:"has_expandable_content"`
}

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

type AssembledPiece struct {
	Layer   ContextLayer `json:"layer"`
	Content string       `json:"content"`
	Tokens  int          `json:"tokens"`
	// Receipt is the engine-minted recall receipt (issue #487) for the
	// memory behind this piece, when receipts are enabled. Additive and
	// omitted when disabled.
	Receipt string `json:"receipt,omitempty"`
}

type AssembledContext struct {
	Query      string           `json:"query"`
	Budget     int              `json:"budget"`
	UsedTokens int              `json:"used_tokens"`
	Pieces     []AssembledPiece `json:"pieces"`

	// #492: hard token budget report (nil when no budget was enforced).
	BudgetReport *BudgetReport `json:"budget_report,omitempty"`

	// #400: expandable session context
	ExpandableSources []ExpandableSource `json:"expandable_sources,omitempty"`

	// #412: delta pack (present when there is a prior snapshot)
	Delta *DeltaPack `json:"delta,omitempty"`
}

// AssemblerOption allows injecting optional dependencies into the Assembler.
type AssemblerOption func(*Assembler)

// WithSummarizer overrides the default summarizer.
func WithSummarizer(s *summarizer.ExtractiveSummarizer) AssemblerOption {
	return func(a *Assembler) {
		a.summarizer = s
	}
}

// ---------------------------------------------------------------------------
// Assembler
// ---------------------------------------------------------------------------

type Assembler struct {
	mu                  sync.Mutex
	database            *db.DB
	summarizer          *summarizer.ExtractiveSummarizer
	embeddings          *extractor.EmbeddingsGenerator
	cfg                 *config.ContextConfig
	workingMemoryConfig *config.WorkingMemoryConfig

	// degradation config (#414)
	degradationCfg *config.DegradationConfig
	// delta-pack config (#412)
	deltaPackCfg *config.DeltaPackConfig
	// profile-layer config (#403)
	profileLayerCfg *config.ProfileLayerConfig
	// session-context config (#400)
	sessionCtxCfg *config.SessionContextConfig
	// recall receipts (#487)
	recallReceipts bool

	// #492: pluggable token estimator for the hard budget
	estimator TokenEstimator

	// #412: per-session snapshot chain
	snapshots map[string][]ContentHashSnapshot
	// #403: cached profile
	cachedProfile *SynthesizedProfile
}

func NewAssembler(database *db.DB, embeddings *extractor.EmbeddingsGenerator, cfg *config.ContextConfig) *Assembler {
	if cfg == nil {
		defaults := config.Defaults()
		cfg = &defaults.Context
	}
	return &Assembler{
		database:   database,
		summarizer: summarizer.NewExtractiveSummarizer(),
		embeddings: embeddings,
		cfg:        cfg,
		snapshots:  make(map[string][]ContentHashSnapshot),
	}
}

// WithTokenEstimator sets a pluggable token estimator for the hard token
// budget (#492). The default is DefaultTokenEstimator; callers with a real
// tokenizer can pass it here so budget enforcement matches the model.
func (a *Assembler) WithTokenEstimator(fn TokenEstimator) *Assembler {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.estimator = fn
	return a
}

// estimate counts tokens for text using the configured estimator (#492).
func (a *Assembler) estimate(text string) int {
	if a.estimator != nil {
		return a.estimator(text)
	}
	return DefaultTokenEstimator(text)
}

// WithDegradationConfig sets the degradation configuration (#414).
func (a *Assembler) WithDegradationConfig(cfg *config.DegradationConfig) *Assembler {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.degradationCfg = cfg
	return a
}

// WithDeltaPackConfig sets the delta-pack configuration (#412).
func (a *Assembler) WithDeltaPackConfig(cfg *config.DeltaPackConfig) *Assembler {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.deltaPackCfg = cfg
	return a
}

// WithProfileLayerConfig sets the profile-layer configuration (#403).
func (a *Assembler) WithProfileLayerConfig(cfg *config.ProfileLayerConfig) *Assembler {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.profileLayerCfg = cfg
	return a
}

// WithSessionContextConfig sets the session-context configuration (#400).
func (a *Assembler) WithSessionContextConfig(cfg *config.SessionContextConfig) *Assembler {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.sessionCtxCfg = cfg
	return a
}

// WithRecallReceipts enables or disables the engine-minted one-line recall
// receipt on retrieval pieces (issue #487). Defaults to off; the MCP and
// CLI entry points mirror the [mcp] recall_receipts config value.
func (a *Assembler) WithRecallReceipts(enabled bool) *Assembler {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.recallReceipts = enabled
	return a
}

// WithFullConfig applies all context-assembler sub-configs from the global config.
func (a *Assembler) WithFullConfig(cfg *config.Config) *Assembler {
	a.WithDegradationConfig(&cfg.Degradation)
	a.WithDeltaPackConfig(&cfg.DeltaPack)
	a.WithProfileLayerConfig(&cfg.ProfileLayer)
	a.WithSessionContextConfig(&cfg.SessionCtx)
	a.WithRecallReceipts(cfg.MCP.RecallReceipts)
	return a
}

func (a *Assembler) SetWorkingMemoryConfig(cfg *config.WorkingMemoryConfig) {
	a.workingMemoryConfig = cfg
}

// ---------------------------------------------------------------------------
// #414: Greedy budget fill with degradation ladder
// ---------------------------------------------------------------------------

// fillRetrievalWithDegradation fills retrieval results greedily by score,
// degrading each piece full→summary→reference before dropping it.
// receiptFor returns the engine-minted recall receipt for a memory when
// receipts are enabled, empty otherwise.
func (a *Assembler) receiptFor(m *db.Memory) string {
	if !a.recallReceipts || m == nil {
		return ""
	}
	return memory.Receipt(m, time.Now())
}

func (a *Assembler) fillRetrievalWithDegradation(results []db.SearchResult, budget int) []AssembledPiece {
	if len(results) == 0 || budget <= 0 {
		return nil
	}

	degCfg := a.degradationCfg
	if degCfg == nil || !degCfg.Enabled {
		// legacy behaviour: one big block
		content := formatRetrievalResults(results)
		tokens := a.estimate(content)
		if tokens <= budget {
			return []AssembledPiece{
				{Layer: LayerRetrieval, Content: content, Tokens: tokens},
			}
		}
		// even the full block doesn't fit — degrade the whole thing
		return degradeRetrievalBlock(results, budget)
	}

	// sort results by score descending (greedy), with a semantic-kind
	// tiebreak within the same score band (#486): identity-level facts
	// (kind user) precede project chatter when scores are close. The
	// retrieval Score already carries the aging decay multiplier (#491).
	sorted := make([]db.SearchResult, len(results))
	copy(sorted, results)
	sort.Slice(sorted, func(i, j int) bool {
		bi, bj := scoreBand(sorted[i].Score), scoreBand(sorted[j].Score)
		if bi != bj {
			return bi > bj
		}
		ki, kj := db.KindRank(sorted[i].Memory.Kind), db.KindRank(sorted[j].Memory.Kind)
		if ki != kj {
			return ki < kj
		}
		return sorted[i].Score > sorted[j].Score
	})

	used := 0
	var pieces []AssembledPiece

	for _, r := range sorted {
		if used >= budget {
			break
		}
		remaining := budget - used

		// Attempt full
		fullContent := formatSingleResult(r, DegradationFull)
		fullTokens := a.estimate(fullContent)
		if fullTokens <= remaining && fullTokens <= degCfg.FullTokens {
			pieces = append(pieces, AssembledPiece{
				Layer:   LayerRetrieval,
				Content: fullContent,
				Tokens:  fullTokens,
				Receipt: a.receiptFor(r.Memory),
			})
			used += fullTokens
			continue
		}

		// Attempt summary
		summaryContent := formatSingleResult(r, DegradationSummary)
		summaryTokens := a.estimate(summaryContent)
		if summaryTokens <= remaining && summaryTokens <= degCfg.SummaryTokens {
			pieces = append(pieces, AssembledPiece{
				Layer:   LayerRetrieval,
				Content: summaryContent,
				Tokens:  summaryTokens,
				Receipt: a.receiptFor(r.Memory),
			})
			used += summaryTokens
			continue
		}

		// Attempt reference
		refContent := formatSingleResult(r, DegradationReference)
		refTokens := a.estimate(refContent)
		if refTokens <= remaining && refTokens <= degCfg.RefTokens {
			pieces = append(pieces, AssembledPiece{
				Layer:   LayerRetrieval,
				Content: refContent,
				Tokens:  refTokens,
				Receipt: a.receiptFor(r.Memory),
			})
			used += refTokens
			continue
		}
		// cannot fit even a reference — drop this item
	}

	return pieces
}

// scoreBand quantizes a retrieval score into 0.05-wide bands so that the
// kind tiebreak only applies within a band ("same score band", #486).
func scoreBand(score float32) int {
	return int(math.Round(float64(score) * 20))
}

// degradeRetrievalBlock formats all results as a single block at degraded level
// when the full block doesn't fit the budget.
func degradeRetrievalBlock(results []db.SearchResult, budget int) []AssembledPiece {
	// fallback: build a compact reference-only block
	var lines []string
	for _, r := range results {
		lines = append(lines, fmt.Sprintf("- [%s] %s", r.Memory.ID, truncateText(r.Memory.Content, 100)))
	}
	content := "Relevant memories (reference):\n" + strings.Join(lines, "\n")
	tokens := estimateTokens(content)
	if tokens > budget {
		// extreme: only fit a few
		var few []string
		for _, r := range results {
			ref := fmt.Sprintf("- %s", truncateText(r.Memory.Content, 40))
			if estimateTokens("Relevant memories:\n"+strings.Join(append(few, ref), "\n")) > budget {
				break
			}
			few = append(few, ref)
		}
		content = "Relevant memories:\n" + strings.Join(few, "\n")
		tokens = estimateTokens(content)
	}
	return []AssembledPiece{
		{Layer: LayerRetrieval, Content: content, Tokens: tokens},
	}
}

// ---------------------------------------------------------------------------
// #412: Delta context packs
// ---------------------------------------------------------------------------

// computeSnapshot computes a content-hash snapshot of the assembled context.
func computeSnapshot(sessionID string, ctx *AssembledContext) ContentHashSnapshot {
	hashes := make([]string, len(ctx.Pieces))
	layerOrder := make([]ContextLayer, len(ctx.Pieces))
	for i, p := range ctx.Pieces {
		h := sha256.Sum256([]byte(p.Content))
		hashes[i] = hex.EncodeToString(h[:])
		layerOrder[i] = p.Layer
	}
	sh := sha256.Sum256([]byte(fmt.Sprintf("%s|%d|%v", sessionID, ctx.UsedTokens, hashes)))
	_ = sh // snapshot identity hash — used as key via Key()

	srcHashes := make(map[string]string)
	for _, es := range ctx.ExpandableSources {
		if es.ContentHash != "" {
			srcHashes[es.SourceID] = es.ContentHash
		}
	}

	return ContentHashSnapshot{
		SessionID:    sessionID,
		TakenAt:      time.Now().UTC(),
		UsedTokens:   ctx.UsedTokens,
		PieceHashes:  hashes,
		LayerOrder:   layerOrder,
		SourceHashes: srcHashes,
	}
}

// snapshotKey returns a hex digest key for a snapshot.
func (s ContentHashSnapshot) Key() string {
	h := sha256.Sum256([]byte(fmt.Sprintf("%s|%d|%v", s.SessionID, s.UsedTokens, s.PieceHashes)))
	return hex.EncodeToString(h[:])
}

// computeDelta computes a DeltaPack between two snapshots.
func computeDelta(sessionID string, prev, curr ContentHashSnapshot) *DeltaPack {
	if prev.Key() == curr.Key() {
		return nil
	}

	dp := &DeltaPack{
		SessionID:    sessionID,
		FromSnapshot: prev.Key(),
		ToSnapshot:   curr.Key(),
		TokenDelta:   curr.UsedTokens - prev.UsedTokens,
	}

	// Build set of previous layer hashes for quick lookup
	prevHashes := make(map[string]bool)
	for _, h := range prev.PieceHashes {
		prevHashes[h] = true
	}

	// Find added pieces (hash not in prev)
	// We reconstruct by layer order — for a real system you'd cache pieces separately.
	// Here we report the layer order difference.
	prevLayers := make(map[ContextLayer]bool)
	for _, l := range prev.LayerOrder {
		prevLayers[l] = true
	}
	currLayers := make(map[ContextLayer]bool)
	for _, l := range curr.LayerOrder {
		currLayers[l] = true
	}

	for l := range prevLayers {
		if !currLayers[l] {
			dp.RemovedLayers = append(dp.RemovedLayers, l)
		}
	}

	// Mark layers that have new content
	for i, l := range curr.LayerOrder {
		if i < len(prev.LayerOrder) && i < len(prev.PieceHashes) {
			if i < len(curr.PieceHashes) && curr.PieceHashes[i] != prev.PieceHashes[i] {
				dp.AddedPieces = append(dp.AddedPieces, AssembledPiece{Layer: l, Tokens: -1})
			}
		} else if !prevLayers[l] {
			dp.AddedPieces = append(dp.AddedPieces, AssembledPiece{Layer: l, Tokens: -1})
		}
	}

	return dp
}

// maybeSnapshot checks whether a new snapshot should be taken based on token
// delta from the last snapshot. It returns a DeltaPack if a new snapshot was
// created and differs from the previous one.
func (a *Assembler) maybeSnapshot(sessionID string, ctx *AssembledContext) *DeltaPack {
	cfg := a.deltaPackCfg
	if cfg == nil || !cfg.Enabled || sessionID == "" {
		return nil
	}

	a.mu.Lock()
	defer a.mu.Unlock()

	chain := a.snapshots[sessionID]
	curr := computeSnapshot(sessionID, ctx)

	// Enforce min interval
	if len(chain) > 0 {
		last := chain[len(chain)-1]
		if curr.UsedTokens < last.UsedTokens {
			// Token count decreased — no meaningful delta
			return nil
		}
		deltaPct := 0
		if last.UsedTokens > 0 {
			deltaPct = ((curr.UsedTokens - last.UsedTokens) * 100) / last.UsedTokens
		}
		if deltaPct < cfg.SnapshotOnTokenPct {
			return nil // not enough change
		}
		interval := curr.TakenAt.Sub(last.TakenAt)
		if interval.Seconds() < float64(cfg.MinSnapshotIntervalSecs) {
			return nil // too soon
		}
	}

	// Append snapshot, enforcing max chain length
	chain = append(chain, curr)
	if len(chain) > cfg.MaxSnapshots {
		chain = chain[len(chain)-cfg.MaxSnapshots:]
	}
	a.snapshots[sessionID] = chain

	if len(chain) < 2 {
		return nil // first snapshot — no delta to compute
	}

	prev := chain[len(chain)-2]
	return computeDelta(sessionID, prev, curr)
}

// Snapshots returns the current snapshot chain for a session (read-only).
func (a *Assembler) Snapshots(sessionID string) []ContentHashSnapshot {
	a.mu.Lock()
	defer a.mu.Unlock()
	chain := a.snapshots[sessionID]
	out := make([]ContentHashSnapshot, len(chain))
	copy(out, chain)
	return out
}

// ---------------------------------------------------------------------------
// #403: Synthesized profile layer
// ---------------------------------------------------------------------------

// synthesizeProfile builds a profile from config/static info and session-derived
// dynamic info, without requiring a search query.
func (a *Assembler) synthesizeProfile(sessionID string) *SynthesizedProfile {
	cfg := a.profileLayerCfg
	if cfg == nil || !cfg.Enabled {
		return nil
	}

	// Use cached profile if available
	if a.cachedProfile != nil {
		return a.cachedProfile
	}

	profile := &SynthesizedProfile{}

	// Static: from database (user prefs etc.)
	if a.database != nil {
		// Attempt to load session-based preferences
		if userPrefs, err := a.loadUserPreferences(); err == nil {
			profile.Static = userPrefs
		}
	}

	// Dynamic: session-derived
	if sessionID != "" && a.database != nil {
		topics, err := a.database.SearchMemoriesBM25("topic preference category", "", 5)
		if err == nil {
			for _, r := range topics {
				if len(profile.Dynamic.RecentTopics) >= 5 {
					break
				}
				content := r.Memory.Content
				if len(content) > 60 {
					content = content[:60]
				}
				profile.Dynamic.RecentTopics = append(profile.Dynamic.RecentTopics, content)
			}
		}
	}

	profile.Dynamic.AccessPattern = "read_write"
	profile.Dynamic.PreferredScopes = []string{"global", "project", "user", "agent"}

	a.cachedProfile = profile
	return profile
}

// loadUserPreferences loads user-level static profile info from the database.
func (a *Assembler) loadUserPreferences() (StaticProfileInfo, error) {
	info := StaticProfileInfo{}

	// Try to get user preferences from memory storage
	prefs, err := a.database.SearchMemoriesBM25("user prefers language role timezone", "", 10)
	if err != nil {
		return info, err
	}
	for _, p := range prefs {
		lower := strings.ToLower(p.Memory.Content)
		if strings.Contains(lower, "language") || strings.Contains(lower, "prefers") {
			if info.Language == "" && len(p.Memory.Content) < 80 {
				info.Language = extractLanguage(p.Memory.Content)
			}
		}
		if strings.Contains(lower, "timezone") || strings.Contains(lower, "time zone") {
			if info.TimeZone == "" {
				info.TimeZone = extractTimezone(p.Memory.Content)
			}
		}
	}
	return info, nil
}

// formatProfile formats a SynthesizedProfile into a string for the profile layer.
func formatProfile(profile *SynthesizedProfile, cfg *config.ProfileLayerConfig) string {
	if profile == nil {
		return ""
	}

	var parts []string

	// Static block
	staticParts := []string{}
	if profile.Static.UserID != "" {
		staticParts = append(staticParts, "user: "+truncateText(profile.Static.UserID, cfg.MaxStaticLen))
	}
	if profile.Static.DisplayName != "" {
		staticParts = append(staticParts, "display: "+truncateText(profile.Static.DisplayName, cfg.MaxStaticLen))
	}
	if profile.Static.Language != "" {
		staticParts = append(staticParts, "lang: "+profile.Static.Language)
	}
	if profile.Static.TimeZone != "" {
		staticParts = append(staticParts, "tz: "+profile.Static.TimeZone)
	}
	if len(staticParts) > 0 {
		parts = append(parts, "[static] "+strings.Join(staticParts, ", "))
	}

	// Dynamic block
	dynParts := []string{}
	if len(profile.Dynamic.RecentTopics) > 0 {
		topics := profile.Dynamic.RecentTopics
		if len(topics) > 5 {
			topics = topics[:5]
		}
		dynParts = append(dynParts, "topics: "+strings.Join(topics, ", "))
	}
	if len(profile.Dynamic.PreferredScopes) > 0 {
		dynParts = append(dynParts, "scopes: "+strings.Join(profile.Dynamic.PreferredScopes, ", "))
	}
	if profile.Dynamic.AccessPattern != "" {
		dynParts = append(dynParts, "access: "+profile.Dynamic.AccessPattern)
	}
	if len(dynParts) > 0 {
		parts = append(parts, "[dynamic] "+strings.Join(dynParts, ", "))
	}

	if len(parts) == 0 {
		return ""
	}
	return "Profile:\n" + strings.Join(parts, "\n")
}

// ---------------------------------------------------------------------------
// #400: Source-linked expandable session context
// ---------------------------------------------------------------------------

// buildSessionContextSourceLinks builds expandable sources from available
// session and memory data.
func (a *Assembler) buildSessionContextSourceLinks(sessionID string) ([]ExpandableSource, error) {
	cfg := a.sessionCtxCfg
	if cfg == nil || !cfg.Enabled {
		return nil, nil
	}

	var sources []ExpandableSource

	if sessionID == "" || a.database == nil {
		return sources, nil
	}

	// Source: session summary
	summary, err := a.database.GetSessionSummary(sessionID)
	if err == nil && summary != "" {
		s := sha256.Sum256([]byte(summary))
		sources = append(sources, ExpandableSource{
			SourceLink: SourceLink{
				SourceID:    sessionID,
				SourceType:  "session",
				Label:       "Session summary",
				ContentHash: hex.EncodeToString(s[:]),
				RefCount:    1,
			},
			ExpandSteps: cfg.MaxExpandPerSrc,
			Metadata:    map[string]string{"type": "summary"},
		})
	}

	// Source: recent memories for this session
	mems, err := a.database.SearchMemoriesBM25("", "", 5)
	if err == nil {
		for _, m := range mems {
			if len(sources) >= cfg.MaxSources {
				break
			}
			contentHash := db.ComputeContentHash(m.Memory.Content)
			sources = append(sources, ExpandableSource{
				SourceLink: SourceLink{
					SourceID:    m.Memory.ID,
					SourceType:  "memory",
					Label:       truncateText(m.Memory.Content, 60),
					ContentHash: contentHash,
					RefCount:    1,
				},
				ExpandSteps: cfg.MaxExpandPerSrc,
				Metadata: map[string]string{
					"scope": m.Memory.Scope,
					"tier":  m.Memory.Tier,
				},
			})
		}
	}

	return sources, nil
}

// formatSessionContext formats expandable sources into a session context layer string.
func formatSessionContext(sources []ExpandableSource) string {
	if len(sources) == 0 {
		return ""
	}
	var sb strings.Builder
	sb.WriteString("Session context sources:\n")
	for _, s := range sources {
		hashSuffix := ""
		if s.ContentHash != "" {
			shortHash := s.ContentHash
			if len(shortHash) > 8 {
				shortHash = shortHash[:8]
			}
			hashSuffix = " [" + shortHash + "]"
		}
		fmt.Fprintf(&sb, "- [%s] %s (type: %s, expands: %d)%s\n",
			s.SourceID, s.Label, s.SourceType, s.ExpandSteps, hashSuffix)
	}
	return sb.String()
}

// ---------------------------------------------------------------------------
// Main Assemble method
// ---------------------------------------------------------------------------

func (a *Assembler) Assemble(query string, sessionText string, sessionID string) (*AssembledContext, error) {
	budget := a.cfg.TokenBudget
	if budget <= 0 {
		budget = 2000
	}
	ctx := &AssembledContext{
		Query:  query,
		Budget: budget,
	}

	usedTokens := 0

	// 1. Working context layer
	if sessionText != "" && a.cfg.MaxWorkingTurns > 0 {
		workingCtx := extractWorkingContext(sessionText, a.cfg.MaxWorkingTurns)
		workingTokens := a.estimate(workingCtx)
		if usedTokens+workingTokens <= budget {
			ctx.Pieces = append(ctx.Pieces, AssembledPiece{
				Layer:   LayerWorkingContext,
				Content: workingCtx,
				Tokens:  workingTokens,
			})
			usedTokens += workingTokens
		}
	}

	// 2. Working memory layer
	if a.workingMemoryConfig != nil && a.workingMemoryConfig.IncludeInContext && usedTokens < budget {
		remaining := budget - usedTokens
		maxItems := a.workingMemoryConfig.MaxItems
		if maxItems <= 0 {
			maxItems = 50
		}
		workingMems, err := a.database.GetWorkingMemories("", maxItems)
		if err == nil && len(workingMems) > 0 {
			security.RedactMemories(workingMems)
			workingMemContent := formatWorkingMemories(workingMems)
			workingMemTokens := a.estimate(workingMemContent)
			if usedTokens+workingMemTokens <= budget {
				ctx.Pieces = append(ctx.Pieces, AssembledPiece{
					Layer:   LayerWorkingMemory,
					Content: workingMemContent,
					Tokens:  workingMemTokens,
				})
				usedTokens += workingMemTokens
			}
			_ = remaining
		}
	}

	// 3. Summary layer
	if sessionID != "" {
		summary, err := a.database.GetSessionSummary(sessionID)
		if err == nil && summary != "" {
			summaryTokens := a.estimate(summary)
			if usedTokens+summaryTokens <= budget {
				ctx.Pieces = append(ctx.Pieces, AssembledPiece{
					Layer:   LayerSummary,
					Content: summary,
					Tokens:  summaryTokens,
				})
				usedTokens += summaryTokens
			}
		}
	}

	// 4. #403: Profile layer (attaches without search query)
	if a.profileLayerCfg != nil && a.profileLayerCfg.Enabled && usedTokens < budget {
		profile := a.synthesizeProfile(sessionID)
		if profile != nil {
			profileContent := formatProfile(profile, a.profileLayerCfg)
			if profileContent != "" {
				profileTokens := a.estimate(profileContent)
				if profileTokens <= a.profileLayerCfg.TokenBudget && usedTokens+profileTokens <= budget {
					ctx.Pieces = append(ctx.Pieces, AssembledPiece{
						Layer:   LayerProfile,
						Content: profileContent,
						Tokens:  profileTokens,
					})
					usedTokens += profileTokens
				}
			}
		}
	}

	// 5. #400: Session context layer with expandable sources
	if a.sessionCtxCfg != nil && a.sessionCtxCfg.Enabled && usedTokens < budget {
		sources, err := a.buildSessionContextSourceLinks(sessionID)
		if err == nil && len(sources) > 0 {
			sessionCtxContent := formatSessionContext(sources)
			if sessionCtxContent != "" {
				sessionTokens := a.estimate(sessionCtxContent)
				if sessionTokens <= a.sessionCtxCfg.TokenBudget && usedTokens+sessionTokens <= budget {
					ctx.Pieces = append(ctx.Pieces, AssembledPiece{
						Layer:   LayerSessionContext,
						Content: sessionCtxContent,
						Tokens:  sessionTokens,
					})
					ctx.ExpandableSources = sources
					usedTokens += sessionTokens
				}
			}
		}
	}

	// 6. #414: Retrieval layer with degradation ladder
	if query != "" && usedTokens < budget {
		remaining := budget - usedTokens
		retrievalResults, err := a.retrieveRelevant(query, remaining)
		if err == nil && len(retrievalResults) > 0 {
			degradedPieces := a.fillRetrievalWithDegradation(retrievalResults, remaining)
			for _, p := range degradedPieces {
				if usedTokens+p.Tokens <= budget {
					ctx.Pieces = append(ctx.Pieces, p)
					usedTokens += p.Tokens
				}
			}
		}
	}

	ctx.UsedTokens = usedTokens

	// #492: hard budget enforcement pass + drop report. The per-layer
	// greedy checks above keep today's behaviour; this pass makes the
	// ceiling authoritative for any estimator and reports what was
	// dropped and why.
	a.enforceBudget(ctx, budget)

	// 7. #412: Delta pack
	if sessionID != "" {
		ctx.Delta = a.maybeSnapshot(sessionID, ctx)
	}

	return ctx, nil
}

// ---------------------------------------------------------------------------
// ProduceSessionSummary
// ---------------------------------------------------------------------------

func (a *Assembler) ProduceSessionSummary(sessionText string, sessionID string) (string, error) {
	if sessionText == "" {
		return "", nil
	}
	summary := a.summarizer.SummarizeSession(sessionText, 5)
	if err := a.database.SaveSessionSummary(sessionID, summary); err != nil {
		return "", fmt.Errorf("failed to save session summary: %w", err)
	}
	return summary, nil
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

func (a *Assembler) retrieveRelevant(query string, tokenBudget int) ([]db.SearchResult, error) {
	if a.embeddings == nil {
		return nil, nil
	}
	emb := a.embeddings.GenerateVector(query)
	queryVec := emb.Vector
	limit := tokenBudget / 50
	if limit < 1 {
		limit = 1
	}
	if limit > 20 {
		limit = 20
	}
	results, err := a.database.SearchMemories(queryVec, emb.Source, "", limit)
	if err != nil {
		return nil, err
	}
	// Defense in depth: content that reaches assembled prompt context must
	// never carry a raw credential, even if it slipped past write-time
	// redaction (#515).
	security.RedactSearchResults(results)
	return results, nil
}

func extractWorkingContext(sessionText string, maxTurns int) string {
	lines := strings.Split(sessionText, "\n")
	if len(lines) <= maxTurns*2 {
		return sessionText
	}
	start := len(lines) - maxTurns*2
	if start < 0 {
		start = 0
	}
	return strings.Join(lines[start:], "\n")
}

// estimateTokens is the legacy word-based heuristic used by tests and
// callers outside the Assembler; the assembler itself routes through the
// pluggable estimator (#492). Kept for backward compatibility.
func estimateTokens(text string) int {
	if text == "" {
		return 0
	}
	words := strings.Fields(text)
	return (len(words) * 4) / 3
}

// formatSingleResult formats a single retrieval result at the given degradation level.
func formatSingleResult(r db.SearchResult, level DegradationLevel) string {
	switch level {
	case DegradationFull:
		return fmt.Sprintf("- %s", r.Memory.Content)
	case DegradationSummary:
		summary := summarizeContent(r.Memory.Content, 80)
		return fmt.Sprintf("- %s [ref: %s]", summary, r.Memory.ID)
	case DegradationReference:
		return fmt.Sprintf("- [%s] %s", r.Memory.ID, extractFirstLine(r.Memory.Content))
	default:
		return fmt.Sprintf("- %s", r.Memory.Content)
	}
}

// formatRetrievalResults formats all results at full level (legacy path).
func formatRetrievalResults(results []db.SearchResult) string {
	var sb strings.Builder
	sb.WriteString("Relevant stored memories:\n")
	for i, r := range results {
		if i >= 10 {
			break
		}
		fmt.Fprintf(&sb, "- %s\n", r.Memory.Content)
	}
	return sb.String()
}

func formatWorkingMemories(mems []*db.Memory) string {
	var sb strings.Builder
	sb.WriteString("Active working memories:\n")
	for i, m := range mems {
		if i >= 20 {
			break
		}
		fmt.Fprintf(&sb, "- %s\n", m.Content)
	}
	return sb.String()
}

// summarizeContent produces a compact summary of a string (extractive first-N-chars approach).
func summarizeContent(content string, maxLen int) string {
	if len(content) <= maxLen {
		return content
	}
	// Try to break at a sentence boundary
	trimmed := content[:maxLen]
	if idx := strings.LastIndex(trimmed, ". "); idx > maxLen/2 {
		return content[:idx+1]
	}
	if idx := strings.LastIndex(trimmed, " "); idx > 0 {
		return content[:idx] + "…"
	}
	return trimmed + "…"
}

// truncateText truncates text to maxLen chars, appending "…" if truncated.
func truncateText(text string, maxLen int) string {
	if len(text) <= maxLen {
		return text
	}
	return text[:maxLen] + "…"
}

// extractFirstLine returns the first line or sentence of content.
func extractFirstLine(content string) string {
	content = strings.TrimSpace(content)
	if idx := strings.Index(content, "\n"); idx > 0 {
		return content[:idx]
	}
	if idx := strings.Index(content, ". "); idx > 0 {
		return content[:idx+1]
	}
	if len(content) > 60 {
		return content[:60] + "…"
	}
	return content
}

// extractLanguage attempts to extract a language code from preference text.
func extractLanguage(text string) string {
	lower := strings.ToLower(text)
	for _, lang := range []string{"english", "german", "french", "spanish", "chinese", "japanese"} {
		if strings.Contains(lower, lang) {
			return lang
		}
	}
	return ""
}

// extractTimezone attempts to extract a timezone from preference text.
func extractTimezone(text string) string {
	knownZones := []string{"UTC", "Europe/Berlin", "America/New_York", "America/Los_Angeles", "Asia/Shanghai", "Asia/Tokyo"}
	for _, z := range knownZones {
		if strings.Contains(text, z) {
			return z
		}
	}
	return ""
}
