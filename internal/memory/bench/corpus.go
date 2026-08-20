// Package bench provides a corpus-backed retrieval evaluation harness for
// measuring BM25, vector, and hybrid search quality and latency.
package bench

import (
	"time"
)

// FixtureMemory is a single memory entry in the benchmark corpus.
type FixtureMemory struct {
	ID        string
	Content   string
	Scope     string            // global, project, agent, user, session
	Metadata  map[string]string // optional metadata
	ValidFrom *time.Time        // optional temporal validity start
	ValidTo   *time.Time        // optional temporal validity end
}

// GroundTruth maps a query to the set of memory IDs that are considered relevant.
type GroundTruth struct {
	Query       string
	RelevantIDs []string // IDs of relevant memories
	Scope       string   // if non-empty, restricts the expected scope
	Description string   // human-readable description of this evaluation case
	Answerable  bool     // false for unanswerable queries used in abstention evaluation
}

// TemporalSlice defines a query for evaluating temporal-validity awareness.
type TemporalSlice struct {
	Query          string
	CurrentlyValid []string // IDs expected to be currently valid
	Expired        []string // IDs expected to be expired (valid_to < now)
	Description    string
}

// ScopeSlice defines a query for evaluating scope isolation.
type ScopeSlice struct {
	Query       string
	Scope       string   // scope to constrain the query to
	ExpectedIDs []string // IDs expected in the results for this scope
	Description string
}

// Corpus holds the complete benchmark fixture.
type Corpus struct {
	Memories       []FixtureMemory
	Queries        []GroundTruth
	TemporalSlices []TemporalSlice
	ScopeSlices    []ScopeSlice
}

// fixedTime creates a fixed UTC time for deterministic fixtures.
func fixedTime(year int, month time.Month, day int) *time.Time {
	t := time.Date(year, month, day, 0, 0, 0, 0, time.UTC)
	return &t
}

// DefaultCorpus returns the built-in deterministic benchmark corpus.
// It contains 50 memories across five categories with 12 evaluation queries,
// 2 temporal-validity slices, and 2 scope-isolation slices.
func DefaultCorpus() *Corpus {
	return &Corpus{
		Memories: defaultMemories(),
		Queries:  defaultQueries(),
		TemporalSlices: []TemporalSlice{
			{
				Query:          "server port configuration",
				CurrentlyValid: []string{"temp-port-active"},
				Expired:        []string{"temp-port-old"},
				Description:    "Only the currently-valid port memory should rank above expired ones",
			},
			{
				Query:          "database path setting",
				CurrentlyValid: []string{"temp-db-active"},
				Expired:        []string{"temp-db-old"},
				Description:    "Only the current database path should be top-ranked",
			},
		},
		ScopeSlices: []ScopeSlice{
			{
				Query:       "dark mode theme preferences",
				Scope:       "user",
				ExpectedIDs: []string{"scope-user-dark-mode"},
				Description: "Querying user scope should only return user-scoped memories",
			},
			{
				Query:       "CI pipeline configuration",
				Scope:       "project",
				ExpectedIDs: []string{"scope-proj-ci", "scope-proj-ci-2"},
				Description: "Querying project scope should only return project-scoped memories",
			},
		},
	}
}

// defaultMemories returns the 50 built-in fixture memories.
func defaultMemories() []FixtureMemory {
	now := time.Now().UTC()
	pastEnd := fixedTime(now.Year()-1, 6, 1)
	futureStart := fixedTime(now.Year()-1, 12, 1)

	return []FixtureMemory{
		// --- Architecture memories (IDs: arch-*) ---
		{ID: "arch-port", Content: "The backend HTTP server listens on port 8080 for the REST API", Scope: "global"},
		{ID: "arch-port-alt", Content: "Alternative deployment uses port 9090 for staging environments", Scope: "global"},
		{ID: "arch-db", Content: "The primary database is SQLite stored at ~/.local/share/symmemory/default.db", Scope: "global"},
		{ID: "arch-embed", Content: "Embeddings use nomic-embed-text via Ollama with FNV-1a hash fallback", Scope: "global"},
		{ID: "arch-mcp", Content: "The MCP server runs over stdio JSON-RPC 2.0 for agent integration", Scope: "global"},
		{ID: "arch-lsh", Content: "LSH bucket pre-filtering avoids full table scans during vector search", Scope: "global"},
		{ID: "arch-wal", Content: "SQLite runs in WAL mode for concurrent reads and writes", Scope: "global"},
		{ID: "arch-pkg", Content: "All database drivers use modernc.org/sqlite for CGO-free compilation", Scope: "global"},
		{ID: "arch-cosine", Content: "Vector similarity uses pure Go cosine similarity calculation", Scope: "global"},
		{ID: "arch-fts", Content: "Full-text search uses SQLite FTS5 with BM25 scoring for keyword retrieval", Scope: "global"},

		// --- Project memories (IDs: proj-*) ---
		{ID: "proj-summarizer", Content: "Extractive summarizer reduces token usage by 60 to 70 percent via sentence extraction", Scope: "project"},
		{ID: "proj-tokens", Content: "Token reduction uses keyword-weighted sentence extraction algorithm", Scope: "project"},
		{ID: "proj-jwt", Content: "JWT authentication uses HMAC-SHA256 with configurable token expiry", Scope: "project"},
		{ID: "proj-pii", Content: "PII guard redacts credit cards emails and API keys before database storage", Scope: "project"},
		{ID: "proj-backup", Content: "Encrypted backup uses AES-256-GCM compression for database exports", Scope: "project"},
		{ID: "proj-web", Content: "Web console is embedded via go:embed and served at localhost 8787", Scope: "project"},
		{ID: "proj-ext", Content: "Chrome browser extension injects memory context into ChatGPT and Claude", Scope: "project"},
		{ID: "proj-tui", Content: "Terminal UI dashboard uses Bubble Tea and Lip Gloss frameworks", Scope: "project"},
		{ID: "proj-import", Content: "Import pipeline supports Claude Code Codex Aider and ChatGPT transcripts", Scope: "project"},
		{ID: "proj-consolidation", Content: "Memory consolidation runs nightly to merge similar facts automatically", Scope: "project"},

		// --- User preference memories (IDs: pref-*) ---
		{ID: "pref-dark-mode", Content: "User prefers dark mode in all applications and interfaces", Scope: "global"},
		{ID: "pref-theme", Content: "User likes minimal clean design with low contrast backgrounds", Scope: "global"},
		{ID: "pref-editor", Content: "User prefers VS Code with vim keybindings for code editing", Scope: "user"},
		{ID: "pref-lang", Content: "User communicates primarily in English with some German technical terms", Scope: "user"},
		{ID: "pref-shell", Content: "User uses zsh shell with oh-my-zsh and powerlevel10k prompt", Scope: "user"},
		{ID: "pref-git", Content: "User prefers conventional commits with conventional-changelog format", Scope: "user"},
		{ID: "pref-testing", Content: "User values high test coverage with table-driven tests in Go", Scope: "user"},
		{ID: "pref-deploy", Content: "User prefers continuous deployment with automated semantic versioning", Scope: "user"},
		{ID: "pref-docs", Content: "User wants documentation in Markdown format with inline code examples", Scope: "user"},
		{ID: "pref-monitoring", Content: "User requires structured logging with JSON output to stderr", Scope: "user"},

		// --- Agent behavior memories (IDs: agent-*) ---
		{ID: "agent-scope", Content: "Memory scoping isolates data between project agent user and session contexts", Scope: "agent"},
		{ID: "agent-stdio", Content: "MCP server transport runs over stdio with no stdout pollution allowed", Scope: "agent"},
		{ID: "agent-rules", Content: "Behavioral rules are stored as procedural instructions for AI agents", Scope: "agent"},
		{ID: "agent-context", Content: "Context assembler combines working context summary and retrieval layers", Scope: "agent"},
		{ID: "agent-retention", Content: "Session memories expire after 30 days by default with configurable TTL", Scope: "agent"},

		// --- Temporal validity memories (IDs: temp-*) ---
		{
			ID: "temp-port-old", Content: "Server runs on port 8080 in production", Scope: "global",
			ValidFrom: fixedTime(now.Year()-2, 1, 1), ValidTo: pastEnd,
		},
		{
			ID: "temp-port-active", Content: "Server runs on port 9090 in production after migration", Scope: "global",
			ValidFrom: futureStart, ValidTo: nil, // no end = still valid
		},
		{
			ID: "temp-db-old", Content: "Database stored at /old/path/symmemory.db", Scope: "global",
			ValidFrom: fixedTime(now.Year()-2, 3, 1), ValidTo: pastEnd,
		},
		{
			ID: "temp-db-active", Content: "Database stored at ~/.local/share/symmemory/default.db after migration", Scope: "global",
			ValidFrom: futureStart, ValidTo: nil,
		},
		{
			ID: "temp-model-old", Content: "Embedding model was text-embedding-ada-002 via OpenAI API", Scope: "global",
			ValidFrom: fixedTime(now.Year()-2, 6, 1), ValidTo: pastEnd,
		},
		{
			ID: "temp-model-active", Content: "Embedding model is nomic-embed-text via local Ollama server", Scope: "global",
			ValidFrom: futureStart, ValidTo: nil,
		},
		{
			ID: "temp-port-transition", Content: "Server port transitioning from 8080 to 9090 during maintenance window", Scope: "global",
			ValidFrom: fixedTime(now.Year()-1, 9, 1), ValidTo: fixedTime(now.Year()-1, 11, 30),
		},
		{
			ID: "temp-db-transition", Content: "Database migration in progress from old path to new XDG default location", Scope: "global",
			ValidFrom: fixedTime(now.Year()-1, 9, 1), ValidTo: fixedTime(now.Year()-1, 11, 30),
		},

		// --- Scope-isolation memories (IDs: scope-*) ---
		{ID: "scope-user-dark-mode", Content: "User prefers dark mode theme in all applications and interfaces", Scope: "user"},
		{ID: "scope-user-font", Content: "User prefers JetBrains Mono font for code editing", Scope: "user"},
		{ID: "scope-proj-ci", Content: "CI pipeline runs Go tests and linting on every push to main", Scope: "project"},
		{ID: "scope-proj-ci-2", Content: "CI pipeline also runs security scanning with gosec and staticcheck", Scope: "project"},
		{ID: "scope-agent-restrict", Content: "Agent must not access files outside its assigned workspace directory", Scope: "agent"},
		{ID: "scope-global-public", Content: "Public documentation lives in the docs directory with Markdown files", Scope: "global"},
		{ID: "scope-session-temp", Content: "Temporary session context for debugging port allocation issue", Scope: "session"},
	}
}

// defaultQueries returns the 12 evaluation queries with ground truth.
func defaultQueries() []GroundTruth {
	return []GroundTruth{
		{
			Query:       "What port does the backend use?",
			RelevantIDs: []string{"arch-port", "arch-port-alt"},
			Description: "Should find the primary port memory and possibly the alt",
		},
		{
			Query:       "dark mode theme preferences",
			RelevantIDs: []string{"pref-dark-mode", "pref-theme", "scope-user-dark-mode"},
			Description: "Should find dark mode and theme preference memories",
		},
		{
			Query:       "SQLite database configuration",
			RelevantIDs: []string{"arch-db", "arch-wal"},
			Description: "Should find database path and WAL mode memories",
		},
		{
			Query:       "embedding model setup",
			RelevantIDs: []string{"arch-embed", "temp-model-active"},
			Description: "Should find embedding model configuration",
		},
		{
			Query:       "session summarization token reduction",
			RelevantIDs: []string{"proj-summarizer", "proj-tokens"},
			Description: "Should find summarizer and token reduction memories",
		},
		{
			Query:       "JWT authentication security",
			RelevantIDs: []string{"proj-jwt", "proj-pii"},
			Description: "Should find JWT and PII guard memories",
		},
		{
			Query:       "scope isolation rules",
			RelevantIDs: []string{"agent-scope", "scope-agent-restrict"},
			Description: "Should find scope isolation and agent restriction memories",
		},
		{
			Query:       "backup encryption export",
			RelevantIDs: []string{"proj-backup"},
			Description: "Should find the encrypted backup memory",
		},
		{
			Query:       "browser extension Chrome integration",
			RelevantIDs: []string{"proj-ext"},
			Description: "Should find the browser extension memory",
		},
		{
			Query:       "MCP server stdio transport",
			RelevantIDs: []string{"arch-mcp", "agent-stdio"},
			Description: "Should find MCP server and stdio transport memories",
		},
		{
			Query:       "conventional commits git workflow",
			RelevantIDs: []string{"pref-git"},
			Description: "Should find the git workflow preference",
		},
		{
			Query:       "context assembler token budget",
			RelevantIDs: []string{"agent-context"},
			Description: "Should find the context assembler memory",
		},
	}
}
