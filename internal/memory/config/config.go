package config

import (
	"github.com/danieljustus/symaira-corekit/configkit"
	"time"
)

// DegradationConfig controls content degradation levels for greedy budget fill.
type DegradationConfig struct {
	Enabled       bool    `json:"enabled"`        // enable degradation ladder (default true)
	FullTokens    int     `json:"full_tokens"`    // budget per full item (default 200)
	SummaryTokens int     `json:"summary_tokens"` // budget per summary (default 80)
	RefTokens     int     `json:"ref_tokens"`     // budget per reference (default 30)
	SummaryRatio  float64 `json:"summary_ratio"`  // fraction of token budget that drops to summary (default 0.3)
}

// DeltaPackConfig controls content-hash based delta context packs.
type DeltaPackConfig struct {
	Enabled                 bool `json:"enabled"`                    // enable delta pack creation (default true)
	MaxSnapshots            int  `json:"max_snapshots"`              // max snapshots to retain per session (default 10)
	SnapshotOnTokenPct      int  `json:"snapshot_on_token_pct"`      // snapshot when used-token delta > this % (default 20)
	MinSnapshotIntervalSecs int  `json:"min_snapshot_interval_secs"` // minimum seconds between snapshots (default 60)
}

// ProfileLayerConfig controls the synthesized static/dynamic profile layer.
type ProfileLayerConfig struct {
	Enabled       bool `json:"enabled"`         // attach profile layer without search query (default true)
	MaxStaticLen  int  `json:"max_static_len"`  // max chars for static profile fields (default 200)
	MaxDynamicLen int  `json:"max_dynamic_len"` // max chars for dynamic/session fields (default 500)
	TokenBudget   int  `json:"token_budget"`    // token budget for profile layer (default 150)
}

// SessionContextConfig controls source-linked expandable session context.
type SessionContextConfig struct {
	Enabled         bool `json:"enabled"`            // enable expandable session context layer (default true)
	MaxSources      int  `json:"max_sources"`        // max expandable sources to attach (default 5)
	TokenBudget     int  `json:"token_budget"`       // token budget for session context layer (default 300)
	MaxExpandPerSrc int  `json:"max_expand_per_src"` // max expand steps per source (default 3)
}

// Config holds all runtime configuration loaded from TOML files.
type Config struct {
	Database      DatabaseConfig       `json:"database"`
	Ollama        OllamaConfig         `json:"ollama"`
	JWT           JWTConfig            `json:"jwt"`
	Security      SecurityConfig       `json:"security"`
	Server        ServerConfig         `json:"server"`
	Consolidation ConsolidationConfig  `json:"consolidation"`
	Ranking       RankingConfig        `json:"ranking"`
	Context       ContextConfig        `json:"context"`
	Degradation   DegradationConfig    `json:"degradation"`
	DeltaPack     DeltaPackConfig      `json:"delta_pack"`
	ProfileLayer  ProfileLayerConfig   `json:"profile_layer"`
	SessionCtx    SessionContextConfig `json:"session_context"`
	Retention     RetentionConfig      `json:"retention"`
	QueryLog      QueryLogConfig       `json:"query_log"`
	HybridSearch  HybridSearchConfig   `json:"hybrid_search"`
	Search        SearchConfig         `json:"search"`
	Import        ImportConfig         `json:"import"`
	WorkingMemory WorkingMemoryConfig  `json:"working_memory"`
	MCP           MCPConfig            `json:"mcp"`
	Memory        MemoryConfig         `json:"memory"`
	Aging         AgingConfig          `json:"aging"`
	Conflict      ConflictConfig       `json:"conflict"`
	PromptMode    string               `json:"prompt_mode"` // "chat" (default) | "code" (#483)
}

type DatabaseConfig struct {
	Path string `json:"path"`
}

type OllamaConfig struct {
	URL   string `json:"url"`
	Model string `json:"model"`
}

type JWTConfig struct {
	SecretPath string `json:"secret_path"`
	Secret     string `json:"secret"`
}

type SecurityConfig struct {
	PIIEnabled     *bool    `json:"pii_enabled"`
	TrustedProxies []string `json:"trusted_proxies"`
	// RequireProfile denies write access to JWT subjects without a stored profile.
	// When false (default), unknown subjects keep default access but a warning is logged.
	RequireProfile bool `json:"require_profile"`
}

type ServerConfig struct {
	HTTPPort int `json:"http_port"`
}

type ConsolidationConfig struct {
	Enabled     bool   `json:"enabled"`
	Schedule    string `json:"schedule"`
	IdleTimeout string `json:"idle_timeout"`
	Timeout     string `json:"timeout"` // HTTP client timeout for LLM calls (e.g. "10m", default "10m")
	Provider    string `json:"provider"`
	Model       string `json:"model"`
	URL         string `json:"url"`
}

// ParseTimeout returns the configured LLM client timeout, defaulting to 10m.
func (c ConsolidationConfig) ParseTimeout() time.Duration {
	if c.Timeout == "" {
		return 10 * time.Minute
	}
	d, err := time.ParseDuration(c.Timeout)
	if err != nil {
		return 10 * time.Minute
	}
	return d
}

// RankingConfig controls retrieval ranking weights.
type RankingConfig struct {
	RelevanceWeight           float64 `json:"relevance_weight"`            // cosine similarity weight (default 0.6)
	RecencyWeight             float64 `json:"recency_weight"`              // recency decay weight (default 0.2)
	ImportanceWeight          float64 `json:"importance_weight"`           // importance weight (default 0.2)
	AccessReinforcementWeight float64 `json:"access_reinforcement_weight"` // access frequency boost (default 0 = disabled)
	RecencyHalfLife           float64 `json:"recency_half_life"`           // half-life in days (default 30)
	AccessHalfLife            float64 `json:"access_half_life"`            // half-life for last-access recency decay (default 14)
	// AccessSpacingHalfLife gates the spacing-aware reinforcement
	// (#489): when access_reinforcement_weight > 0 and a memory carries
	// prev_access data, the count boost scales with the interval since the
	// previous reinforcement (same-session bursts hit diminishing returns).
	// Default 30 days.
	AccessSpacingHalfLife float64 `json:"access_spacing_half_life"`
	// SpreadingWeight enables the memory-to-memory association bonus
	// (#488): memories up to 2 hops away from a strong retrieval hit gain
	// score so facts that are only relevant through association can
	// surface. Default 0 = disabled (rankings stay byte-identical until
	// explicitly enabled; any default change must be validated with
	// `symmemory bench` first).
	SpreadingWeight float64 `json:"spreading_weight"`
}

// ContextConfig controls the context assembler.
type ContextConfig struct {
	TokenBudget          int `json:"token_budget"`           // max tokens for assembled context (default 2000)
	WorkingContextTokens int `json:"working_context_tokens"` // budget for working context (default 500)
	SummaryTokens        int `json:"summary_tokens"`         // budget for session summary (default 500)
	RetrievalTokens      int `json:"retrieval_tokens"`       // budget for semantic retrieval (default 1000)
	MaxWorkingTurns      int `json:"max_working_turns"`      // max recent turns to include (default 5)
}

// RetentionConfig controls data lifecycle governance.
type RetentionConfig struct {
	SessionTTL         string `json:"session_ttl"`          // e.g. "24h", "7d" (default "720h" = 30d)
	AutoPurgeEnabled   bool   `json:"auto_purge_enabled"`   // enable background purge (default false)
	AuditLogEnabled    bool   `json:"audit_log_enabled"`    // enable audit logging (default true)
	AuditRetention     string `json:"audit_retention"`      // how long to keep audit logs (default "720h")
	ActivitySegmentTTL string `json:"activity_segment_ttl"` // activity segment retention (default "48h")
	ActivityEpisodeTTL string `json:"activity_episode_ttl"` // activity episode retention (default "720h")
}

// QueryLogConfig controls query log retention policy (issue #457) and the
// result-set recording switch (issue #460).
// The query log is bounded so it cannot grow without limit; the bounds are
// deliberate and configurable here.
type QueryLogConfig struct {
	// MaxEntries is the row cap for the query log. When the table exceeds
	// it, the oldest entries are pruned on write. 0 or a negative value
	// means "use the default" (1000), which preserves the historical
	// behavior when no [query_log] section is present.
	MaxEntries int `json:"max_entries"`
	// MaxAge optionally caps how long entries are kept (e.g. "720h", "7d").
	// Entries older than this are pruned on write. Empty disables age-based
	// pruning.
	MaxAge string `json:"max_age"`
	// RecordResults controls whether each retrieval records the memories it
	// returned (ids and scores only — references, never content) in
	// query_log_results. Defaults to true; set to false to opt out.
	RecordResults bool `json:"record_results"`
}

// HybridSearchConfig controls hybrid vector + BM25 retrieval.
type HybridSearchConfig struct {
	Enabled          bool    `json:"enabled"`            // enable hybrid search (default true)
	BM25Weight       float64 `json:"bm25_weight"`        // BM25 weight in fusion (default 0.3)
	VectorWeight     float64 `json:"vector_weight"`      // vector weight in fusion (default 0.7)
	MMREnabled       bool    `json:"mmr_enabled"`        // enable MMR diversity (default false)
	MMRLambda        float64 `json:"mmr_lambda"`         // MMR lambda (0=diversity, 1=relevance, default 0.7)
	QuantizeToBinary bool    `json:"quantize_to_binary"` // store sign-bit binary vectors for fast Hamming prefilter
	PrefilterEnabled bool    `json:"prefilter_enabled"`  // use Hamming prefilter before cosine scoring (opt-in)
	SparsemaxEnabled bool    `json:"sparsemax_enabled"`  // apply sparsemax (α=2) to fused scores (default false)
	PerArmMultiplier int     `json:"per_arm_multiplier"` // per-arm result cap multiplier before fusion (default 3)
}

// SearchConfig controls default retrieval behavior for memory_search / search.
type SearchConfig struct {
	MinScore                  float64 `json:"min_score"`                   // minimum similarity score; results below are dropped (default 0 = disabled)
	TemporalExtractionEnabled bool    `json:"temporal_extraction_enabled"` // enable inference of time window constraints from query text (default false)
}

// ImportConfig holds per-tool import settings.
type ImportConfig struct {
	Tools           map[string]ImportToolConfig `json:"tools"`
	ExtractOnImport bool                        `json:"extract_on_import"` // run extraction/summarization on transcript imports
}

// ImportToolConfig holds configuration for a single importer.
type ImportToolConfig struct {
	Path    string            `json:"path"`
	Token   string            `json:"token"`
	Options map[string]string `json:"options"`
}

// WorkingMemoryConfig controls the working-memory tier lifecycle.
type WorkingMemoryConfig struct {
	TTL              string `json:"ttl"`                // expiration duration for working memories (default "24h")
	MaxItems         int    `json:"max_items"`          // max working memories in context assembly (default 50)
	IncludeInContext bool   `json:"include_in_context"` // include working memories in assembled context (default true)
}

// MCPConfig controls the MCP stdio server behavior.
type MCPConfig struct {
	// ClientID pins the attribution identity (created_by/updated_by) for all
	// MCP writes, winning over the client identity captured from the
	// initialize handshake. Empty means "derive from the handshake, fall
	// back to 'mcp' when nothing is resolvable".
	ClientID string `json:"client_id"`
	// RecallReceipts toggles the engine-minted one-line recall receipt
	// attached to each returned memory in MCP search results and context
	// assembly pieces (issue #487). Defaults to true; set to false to drop
	// the additive field.
	RecallReceipts bool `json:"recall_receipts"`
}

// MemoryConfig controls the write path (#485): staging of autonomous
// writes. Staging is opt-in per client via the memory_set `staged`
// parameter until an operator flips StageWritesByDefault for low-trust
// clients; explicit per-call flags always win.
type MemoryConfig struct {
	// StageWritesByDefault makes memory_set writes land as staged
	// candidates (excluded from retrieval) unless the caller explicitly
	// passes staged=false. Default false: writes stay live.
	StageWritesByDefault bool `json:"stage_writes_by_default"`
}

// AgingConfig controls the aging pass (#491). Decay is a multiplier in
// (0,1] applied to retrieval scores; retirement below RetireBelow flags a
// memory (never deletes it). Setting access_half_life_days to 0 or
// enabled=false disables decay effectively.
type AgingConfig struct {
	Enabled            bool    `json:"enabled"`
	AccessHalfLifeDays float64 `json:"access_half_life_days"`
	RetireBelow        float64 `json:"retire_below"`
	AccessBoostCap     int64   `json:"access_boost_cap"`
}

// ConflictConfig controls write-path contradiction detection (#462).
//
// When enabled, each long-term write is compared against prior memories in
// the same scope: byte-identical repeats and near-duplicates at or above
// NearDupThreshold are deduplicated (no second row), candidates in the
// [ContradictionThreshold, NearDupThreshold) band get a verdict, and a
// contradiction resolves by marking the loser superseded (superseded_by +
// closed valid_to) with an audit event naming both memories, both actors
// and the deciding rule. Undecidable pairs are stored unchanged and
// surfaced via a conflict_pending audit event — a silently wrong
// supersession is worse than a visible conflict. Disabling the check
// restores the exact legacy write behavior.
type ConflictConfig struct {
	// Enabled turns the whole check off. Default true.
	Enabled bool `json:"enabled"`
	// ContradictionThreshold is the cosine similarity at/above which a
	// same-scope candidate is a potential contradiction and gets a
	// verdict. Below it, candidates are unrelated and stored alongside.
	// Default 0.80.
	ContradictionThreshold float64 `json:"contradiction_threshold"`
	// NearDupThreshold is the cosine similarity at/above which a
	// candidate is the same fact as the new content (a repeat that gets
	// deduplicated instead of litigated as a conflict). It matches the
	// consolidation engine's same-fact threshold so the write path and
	// consolidation never disagree about what "the same fact" means.
	// Default 0.95.
	NearDupThreshold float64 `json:"near_dup_threshold"`
	// MaxCandidates caps how many same-scope candidates are recalled per
	// write for the contradiction check. Default 10.
	MaxCandidates int `json:"max_candidates"`
	// LLMProvider ("ollama" or "openai") enables the optional LLM verdict
	// tier that classifies contradiction-band pairs as repeat,
	// contradiction or ambiguous. Empty (default) keeps verdicts purely
	// deterministic — hash and cosine tiers only — so a CLI write never
	// requires an LLM round-trip.
	LLMProvider string `json:"llm_provider"`
	// LLMModel is the model used by the LLM verdict tier.
	LLMModel string `json:"llm_model"`
	// LLMURL is the OpenAI-compatible endpoint for the LLM verdict tier.
	LLMURL string `json:"llm_url"`
}

// Defaults returns a Config with sensible default values.
func Defaults() *Config {
	trueVal := true
	return &Config{
		Ollama: OllamaConfig{
			URL:   "http://localhost:11434/api/embeddings",
			Model: "nomic-embed-text",
		},
		Security: SecurityConfig{
			PIIEnabled:     &trueVal,
			RequireProfile: false,
		},
		Server: ServerConfig{
			HTTPPort: 0,
		},
		MCP: MCPConfig{
			RecallReceipts: true,
		},
		Memory: MemoryConfig{
			StageWritesByDefault: false,
		},
		Aging: AgingConfig{
			Enabled:            true,
			AccessHalfLifeDays: 120,
			RetireBelow:        0.1,
			AccessBoostCap:     20,
		},
		Conflict: ConflictConfig{
			Enabled:                true,
			ContradictionThreshold: 0.80,
			NearDupThreshold:       0.95,
			MaxCandidates:          10,
		},
		PromptMode: "chat", // #483: chat | code; chat stays the unchanged default
		Consolidation: ConsolidationConfig{
			Enabled:     true,
			Schedule:    "0 2 * * *",
			IdleTimeout: "30m",
			Timeout:     "10m",
			Provider:    "",
			Model:       "",
		},
		Ranking: RankingConfig{
			RelevanceWeight:           0.6,
			RecencyWeight:             0.2,
			ImportanceWeight:          0.2,
			AccessReinforcementWeight: 0.0,
			RecencyHalfLife:           30,
			AccessHalfLife:            14,
			AccessSpacingHalfLife:     30,
			SpreadingWeight:           0.0,
		},
		Context: ContextConfig{
			TokenBudget:          2000,
			WorkingContextTokens: 500,
			SummaryTokens:        500,
			RetrievalTokens:      1000,
			MaxWorkingTurns:      5,
		},
		Degradation: DegradationConfig{
			Enabled:       true,
			FullTokens:    200,
			SummaryTokens: 80,
			RefTokens:     30,
			SummaryRatio:  0.3,
		},
		DeltaPack: DeltaPackConfig{
			Enabled:                 true,
			MaxSnapshots:            10,
			SnapshotOnTokenPct:      20,
			MinSnapshotIntervalSecs: 60,
		},
		ProfileLayer: ProfileLayerConfig{
			Enabled:       true,
			MaxStaticLen:  200,
			MaxDynamicLen: 500,
			TokenBudget:   150,
		},
		SessionCtx: SessionContextConfig{
			Enabled:         true,
			MaxSources:      5,
			TokenBudget:     300,
			MaxExpandPerSrc: 3,
		},
		Retention: RetentionConfig{
			SessionTTL:         "720h",
			AutoPurgeEnabled:   false,
			AuditLogEnabled:    true,
			AuditRetention:     "720h",
			ActivitySegmentTTL: "48h",
			ActivityEpisodeTTL: "720h",
		},
		QueryLog: QueryLogConfig{
			MaxEntries:    1000,
			MaxAge:        "",
			RecordResults: true,
		},
		HybridSearch: HybridSearchConfig{
			Enabled:          true,
			BM25Weight:       0.3,
			VectorWeight:     0.7,
			MMREnabled:       false,
			MMRLambda:        0.7,
			SparsemaxEnabled: false,
			PerArmMultiplier: 3,
		},
		Import: ImportConfig{
			ExtractOnImport: true,
		},
		Search: SearchConfig{
			MinScore: 0,
		},
		WorkingMemory: WorkingMemoryConfig{
			TTL:              "24h",
			MaxItems:         50,
			IncludeInContext: true,
		},
	}
}

var loader = configkit.NewLoader[Config](
	configkit.Options{
		AppName:   "symmemory",
		EnvPrefix: "SYMMEMORY",
	},
	Defaults,
)

// Load reads the global config from ~/.config/symmemory/config.toml,
// then merges a project-level .symmemory.toml override if present.
// The config is loaded once and cached for subsequent calls.
func Load() (*Config, error) {
	return loader.Load()
}

// Reload reads a fresh config from disk (global + project files) and applies
// environment variable overrides. Unlike Load, it never returns a cached value.
// Intended for long-running servers that need to pick up config changes without
// restarting.
func Reload() (*Config, error) {
	return loader.Reload()
}

// resetCache clears the cached config so the next Load() call reads from disk again.
// It is used only by tests.
func resetCache() {
	loader.ResetCache()
}
