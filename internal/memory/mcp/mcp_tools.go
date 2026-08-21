package mcp

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strconv"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/instructions"
	"github.com/danieljustus/symaira-brain/internal/memory/memory"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
	"github.com/danieljustus/symaira-brain/internal/memory/temporal"
	"github.com/danieljustus/symaira-corekit/mcpserver"
)

type MemoryResponse struct {
	ID                  string            `json:"id"`
	Content             string            `json:"content"`
	Scope               string            `json:"scope"`
	Metadata            map[string]string `json:"metadata,omitempty"`
	CreatedAt           time.Time         `json:"created_at"`
	UpdatedAt           time.Time         `json:"updated_at"`
	CreatedBy           string            `json:"created_by,omitempty"`
	CreatedSession      string            `json:"created_session,omitempty"`
	Entities            []string          `json:"entities,omitempty"`
	ConsolidationStatus string            `json:"consolidation_status,omitempty"`
	EmbeddingSource     string            `json:"embedding_source,omitempty"`
	EmbeddingModel      string            `json:"embedding_model,omitempty"`
	Importance          float64           `json:"importance,omitempty"`
	Evidence            []db.EvidenceSpan `json:"evidence,omitempty"`
}

type SearchResultResponse struct {
	Memory        MemoryResponse `json:"memory"`
	Score         float32        `json:"score"`
	SourceProfile string         `json:"source_profile,omitempty"`
	SourceScope   string         `json:"source_scope,omitempty"`
	// Receipt is the engine-minted recall receipt (issue #487), additive
	// and omitted when receipts are disabled.
	Receipt string `json:"receipt,omitempty"`
}

func memoryResponse(m *db.Memory) MemoryResponse {
	if m == nil {
		return MemoryResponse{}
	}
	// Defense in depth: re-redact at the response boundary so a record that
	// reached storage without write-time redaction (legacy import, or a
	// gap in the write-time patterns) never leaves the MCP transport with
	// raw credential-shaped content (#515).
	security.RedactMemory(m)
	return MemoryResponse{
		ID:                  m.ID,
		Content:             m.Content,
		Scope:               m.Scope,
		Metadata:            m.Metadata,
		CreatedAt:           m.CreatedAt,
		UpdatedAt:           m.UpdatedAt,
		CreatedBy:           m.CreatedBy,
		CreatedSession:      m.CreatedSession,
		Entities:            m.Entities,
		ConsolidationStatus: m.ConsolidationStatus,
		EmbeddingSource:     m.EmbeddingSource,
		EmbeddingModel:      m.EmbeddingModel,
		Importance:          m.Importance,
	}
}

func searchResultResponse(r db.SearchResult) SearchResultResponse {
	return SearchResultResponse{
		Memory:        memoryResponse(r.Memory),
		Score:         r.Score,
		SourceProfile: r.SourceProfile,
		SourceScope:   r.SourceScope,
	}
}

// hybridResultsToSearchResults converts HybridResult slices to SearchResult
// slices, using FusedScore as the result Score for downstream formatting.
func hybridResultsToSearchResults(hybrid []db.HybridResult) []db.SearchResult {
	results := make([]db.SearchResult, len(hybrid))
	for i, h := range hybrid {
		results[i] = db.SearchResult{
			Memory: h.Memory,
			Score:  float32(h.FusedScore),
		}
	}
	return results
}

func (s *Server) MCPServer() *mcpserver.Server {
	srv := mcpserver.New("symaira-memory", s.version)
	srv.SetInstructions(instructions.Text(s.version))
	s.RegisterTools(srv, nil)
	return srv
}

// RegisterTools registers symmemory MCP tools onto srv, filtered by
// the allowed set (nil = all). The brain gateway passes the
// policy-exposed tool names (policy.EvaluatePreset) so mode presets and
// tools_deny are respected; stand-alone symmemory (MCPServer) passes nil.
func (s *Server) RegisterTools(srv *mcpserver.Server, allowed map[string]bool) {
	if allowed == nil || allowed["memory_get"] {
		srv.RegisterTool(&mcpserver.Tool{
			Name:        "memory_get",
			Description: "Retrieve a specific memory by its unique ID.",
			InputSchema: json.RawMessage(`{"type":"object","properties":{"id":{"type":"string","description":"Unique memory UUID"},"client_id":{"type":"string","description":"Optional client ID for access control filtering"},"with_evidence":{"type":"boolean","description":"Optional: include grounded evidence spans backing this memory, if any (default false)"}},"required":["id"]}`),
			Handler:     s.handleMemoryGet,
		},
		)
	}

	if allowed == nil || allowed["memory_set"] {
		srv.RegisterTool(&mcpserver.Tool{
			Name:        "memory_set",
			Description: "Save a new persistent memory or fact. Use this tool autonomously when the user expresses a clear preference, constraint, architectural decision, or guideline that should persist across sessions. The 'kind' parameter is REQUIRED: classify the fact as one of user (preferences, personal facts), feedback (corrections, evaluations), project (rules, constraints, architectural decisions), or reference (external facts, documentation). Use 'staged': true when the fact was derived autonomously without explicit user confirmation — it is then held as a candidate that does not affect retrieval until a human reviews it.",
			InputSchema: json.RawMessage(`{"type":"object","properties":{"content":{"type":"string","description":"The text content or fact to remember (e.g., 'User prefers TypeScript for script tasks' or 'API uses port 8080'). Keep it concise and objective."},"kind":{"type":"string","description":"REQUIRED semantic kind: 'user' (preferences, personal facts), 'feedback' (corrections, evaluations), 'project' (rules, constraints, architectural decisions), 'reference' (external facts, documentation). Synonyms are accepted."},"scope":{"type":"string","description":"Scope level: 'global' (default, for general user settings), 'project' (highly recommended for folder-specific codebases; auto-resolves project name using .symmemory.toml or .git in CWD), 'agent', 'user', or 'session'"},"metadata":{"type":"string","description":"Optional JSON metadata key-value string (e.g., '{\"source\": \"claude-agent\"}')"},"session_id":{"type":"string","description":"Optional session ID for provenance tracking (e.g., the current chat/conversation session identifier)"},"entities":{"type":"string","description":"Optional comma-separated entity names to link (e.g., 'Irene,Premium BnB'). Entities are auto-created if they don't exist."},"working":{"type":"boolean","description":"Store as working memory with TTL-based eviction (default false)"},"staged":{"type":"boolean","description":"Store as a staged candidate (excluded from retrieval until reviewed/promoted). Default false; when the server is configured with stage_writes_by_default, writes default to staged unless staged=false is passed explicitly."}},"required":["content","kind"]}`),
			Handler:     s.handleMemorySet,
		},
		)
	}

	if allowed == nil || allowed["memory_candidates"] {
		srv.RegisterTool(&mcpserver.Tool{
			Name:        "memory_candidates",
			Description: "List memories that were written as staged candidates and still await review (promote/reject). Staged candidates never affect retrieval, so this is the only way to see them.",
			InputSchema: json.RawMessage(`{"type":"object","properties":{"limit":{"type":"integer","description":"Optional maximum number of candidates to return (default 100)"}}}`),
			Handler:     s.handleMemoryCandidates,
		},
		)
	}

	if allowed == nil || allowed["memory_promote"] {
		srv.RegisterTool(&mcpserver.Tool{
			Name:        "memory_promote",
			Description: "Approve a staged candidate memory so it becomes retrievable by search and context assembly. Use after a human confirmed the fact.",
			InputSchema: json.RawMessage(`{"type":"object","properties":{"id":{"type":"string","description":"Unique memory UUID of the staged candidate"}},"required":["id"]}`),
			Handler:     s.handleMemoryPromote,
		},
		)
	}

	if allowed == nil || allowed["memory_reject"] {
		srv.RegisterTool(&mcpserver.Tool{
			Name:        "memory_reject",
			Description: "Discard a staged candidate memory (deletes it). Refuses to act on memories that are not staged candidates.",
			InputSchema: json.RawMessage(`{"type":"object","properties":{"id":{"type":"string","description":"Unique memory UUID of the staged candidate"}},"required":["id"]}`),
			Handler:     s.handleMemoryReject,
		},
		)
	}

	if allowed == nil || allowed["memory_search"] {
		srv.RegisterTool(&mcpserver.Tool{
			Name:        "memory_search",
			Description: "Perform a semantic vector similarity search on stored memories. Always use this tool at the start of a session or task to retrieve relevant past design decisions, user preferences, and project guidelines.",
			InputSchema: json.RawMessage(`{"type":"object","properties":{"query":{"type":"string","description":"The natural language query or semantic term (e.g., 'database port' or 'language preference')"},"scope":{"type":"string","description":"Optional scope level filter ('global', 'project', 'agent', 'user', 'session')"},"session_id":{"type":"string","description":"Optional session ID recorded in the query log for attribution (e.g. the current chat/conversation session identifier)"},"profile":{"type":"string","description":"Optional context profile name for inherited scope resolution. When provided, searches across scopes defined by the profile in precedence order."},"limit":{"type":"integer","description":"Optional maximum number of search results to return (default 5)"},"entity":{"type":"string","description":"Optional entity name filter — only returns memories linked to this entity"},"min_confidence":{"type":"string","description":"Optional minimum confidence level filter ('low', 'medium', 'high')"},"verification":{"type":"string","description":"Optional verification status filter ('verified', 'unverified', 'stale')"},"exclude_superseded":{"type":"boolean","description":"Optional exclude memories that have been superseded (default false)"},"max_age":{"type":"string","description":"Optional maximum memory age (e.g. '7d', '30d', '1y')"},"max_sensitivity":{"type":"string","description":"Optional maximum sensitivity level ('public', 'internal', 'confidential', 'secret')"},"min_sharing_level":{"type":"string","description":"Optional minimum sharing level ('private', 'team', 'org', 'public')"},"client_id":{"type":"string","description":"Optional client ID for access control filtering"},"with_evidence":{"type":"boolean","description":"Optional: include grounded evidence spans for each result, if any (default false)"},"min_score":{"type":"number","description":"Optional minimum similarity score (0-1). Results below the threshold are dropped and the tool returns an explicit 'no confident match' marker instead of weak matches. Defaults to the search.min_score config value; 0 disables filtering."},"max_payload_bytes":{"type":"integer","description":"Optional maximum payload size in bytes for the search response. When exceeded, results are truncated to fit."},"cursor":{"type":"string","description":"Optional pagination cursor returned by a previous search/list response."},"from":{"type":"string","description":"Optional RFC3339 or YYYY-MM-DD timestamp: only return memories valid at or after this time (filters against valid_to column)"},"to":{"type":"string","description":"Optional RFC3339 or YYYY-MM-DD timestamp: only return memories valid at or before this time (filters against valid_from column)"}},"required":["query"]}`),

			Handler: s.handleMemorySearch,
		},
		)
	}

	if allowed == nil || allowed["memory_list"] {
		srv.RegisterTool(&mcpserver.Tool{
			Name:        "memory_list",
			Description: "List all memories currently stored in the database. Useful for debugging or displaying stored context lists.",
			InputSchema: json.RawMessage(`{"type":"object","properties":{"scope":{"type":"string","description":"Optional scope level filter ('global', 'project', 'agent', 'user', 'session')"},"limit":{"type":"integer","description":"Optional maximum number of memories to return (default 100, max 1000)"},"max_sensitivity":{"type":"string","description":"Optional maximum sensitivity level ('public', 'internal', 'confidential', 'secret')"},"min_sharing_level":{"type":"string","description":"Optional minimum sharing level ('private', 'team', 'org', 'public')"},"client_id":{"type":"string","description":"Optional client ID for access control filtering"},"as_of":{"type":"string","description":"Optional RFC3339 timestamp: return memory state as of this point in time instead of current state. Not combinable with the policy filters."},"max_payload_bytes":{"type":"integer","description":"Optional maximum total response payload size in bytes (e.g. 100000 for ~100KB). Results are truncated to stay within the limit. Default 0 (no limit)."},"cursor":{"type":"string","description":"Optional cursor for pagination — pass the cursor returned by a previous call to get the next page of results."}}}`),
			Handler:     s.handleMemoryList,
		},
		)
	}

	if allowed == nil || allowed["entity_list"] {
		srv.RegisterTool(&mcpserver.Tool{
			Name:        "entity_list",
			Description: "List all known entities (people, projects, organizations). Use this to discover which entities exist before linking memories or filtering searches.",
			InputSchema: json.RawMessage(`{"type":"object","properties":{}}`),
			Handler:     s.handleEntityList,
		},
		)
	}

	if allowed == nil || allowed["entity_relate"] {
		srv.RegisterTool(&mcpserver.Tool{
			Name:        "entity_relate",
			Description: "Create or delete a directed, typed relationship between two entities (e.g. 'Alice works-with Bob'), by name or by stable entity ID. Use action='delete' to remove a relation. Pass source/source_ref/verification/evidence to attach provenance for idempotent creation by external integrations — retrying the same source+source_ref+triple returns the existing relation, and an already-verified relation is never silently overwritten. Pass valid_from/valid_until to attach temporal validity intervals; creating a newer version of the same triple with valid_from updates the row in place and makes the previous interval invisible to as-of queries.",
			InputSchema: json.RawMessage(`{"type":"object","properties":{"from":{"type":"string","description":"Name or alias of the source entity (mutually exclusive with from_id)"},"to":{"type":"string","description":"Name or alias of the target entity (mutually exclusive with to_id)"},"from_id":{"type":"string","description":"Stable ID of the source entity (mutually exclusive with from)"},"to_id":{"type":"string","description":"Stable ID of the target entity (mutually exclusive with to)"},"relation":{"type":"string","description":"Relation type, free-form (e.g. 'works-with', 'manages', 'attended')"},"action":{"type":"string","description":"'create' (default) or 'delete'"},"source":{"type":"string","description":"Optional caller-supplied source identifier for idempotent provenance (e.g. 'symdesk'); required together with source_ref"},"source_ref":{"type":"string","description":"Optional opaque caller reference for idempotency (e.g. a meeting ID; never an absolute path); required together with source"},"verification":{"type":"string","description":"Optional provenance verification status: 'verified' or 'unverified'"},"evidence":{"type":"string","description":"Optional bounded evidence JSON: {source_doc_id, char_start, char_end, time_start, time_end}"},"valid_from":{"type":"string","description":"Optional start of validity interval (RFC3339 or YYYY-MM-DD). When provided on an existing triple, updates the temporal window."},"valid_until":{"type":"string","description":"Optional end of validity interval (RFC3339 or YYYY-MM-DD). NULL means open-ended."}},"required":["relation"]}`),
			Handler:     s.handleEntityRelate,
		},
		)
	}

	if allowed == nil || allowed["entity_resolve"] {
		srv.RegisterTool(&mcpserver.Tool{
			Name:        "entity_resolve",
			Description: "Return deterministic, explainable entity candidates for a name or alias query — scored and ranked, with the match reason for each. Use this instead of entity_relate/graph_neighbors' implicit lookup when a caller needs to see or disambiguate multiple possible matches before acting (e.g. mapping an external record to an existing entity) rather than silently picking one.",
			InputSchema: json.RawMessage(`{"type":"object","properties":{"query":{"type":"string","description":"Name or alias to resolve"},"type":{"type":"string","description":"Optional: restrict candidates to this exact entity type (person, project, org, other)"},"aliases":{"type":"string","description":"Optional comma-separated alias hints to also compare against (never stored; hints shaped like an email or phone number are dropped)"},"limit":{"type":"integer","description":"Optional maximum number of candidates to return (default 10)"}},"required":["query"]}`),
			Handler:     s.handleEntityResolve,
		},
		)
	}

	if allowed == nil || allowed["graph_neighbors"] {
		srv.RegisterTool(&mcpserver.Tool{
			Name:        "graph_neighbors",
			Description: "Return the entities and relations reachable from a starting entity via a breadth-first traversal, as {nodes, edges}. Use this to answer 'what connects to X'. Pass as_of to filter relations by validity at a specific point in time.",
			InputSchema: json.RawMessage(`{"type":"object","properties":{"entity":{"type":"string","description":"Name or alias of the starting entity"},"depth":{"type":"integer","description":"Traversal depth, 1-3 (default 1)"},"as_of":{"type":"string","description":"Optional RFC3339 or YYYY-MM-DD timestamp: only return relations valid at this point in time (default: now)"}},"required":["entity"]}`),
			Handler:     s.handleGraphNeighbors,
		},
		)
	}

	if allowed == nil || allowed["query_log"] {
		srv.RegisterTool(&mcpserver.Tool{
			Name:        "query_log",
			Description: "Return a summary of recent MCP tool calls (query log). Shows tool, actor and per-actor breakdown plus recent entries. Read-only, no side effects.",
			InputSchema: json.RawMessage(`{"type":"object","properties":{"limit":{"type":"integer","description":"Optional maximum number of recent entries to return (default 20)"},"actor":{"type":"string","description":"Optional actor (client identity) filter — narrows the summary and recent entries to queries recorded for this actor"}}}`),
			Handler:     s.handleQueryLog,
		},
		)
	}
}

func (s *Server) handleMemoryGet(ctx context.Context, input json.RawMessage) (any, error) {
	var args struct {
		ID           string `json:"id"`
		ClientID     string `json:"client_id"`
		WithEvidence bool   `json:"with_evidence"`
	}
	if err := json.Unmarshal(input, &args); err != nil {
		return nil, fmt.Errorf("invalid arguments for 'memory_get': failed to parse arguments: %w", err)
	}
	if args.ID == "" {
		return nil, fmt.Errorf("invalid arguments for 'memory_get': 'id' is required")
	}

	m, err := s.service.Get(args.ID)
	if err != nil {
		if nf, ok := err.(*NotFoundError); ok {
			return nf.Error(), nil
		}
		return mcpError("Failed to fetch memory", err)
	}

	if args.ClientID != "" {
		policyFilter := db.PolicyFilter{ClientID: args.ClientID}
		if !db.PassesPolicyFilter(m, policyFilter) {
			return nil, fmt.Errorf("access denied: memory %s is not accessible by client %s", args.ID, args.ClientID)
		}
	}

	resp := memoryResponse(m)
	if args.WithEvidence {
		evidence, err := s.service.GetMemoryEvidence(args.ID)
		if err != nil {
			return mcpError("Failed to fetch memory evidence", err)
		}
		resp.Evidence = evidence
	}

	data, _ := json.MarshalIndent(resp, "", "  ")
	return string(data), nil
}

func (s *Server) handleMemorySet(ctx context.Context, input json.RawMessage) (any, error) {
	s.ensureAuditLogConfig()
	if s.profile != nil && !security.ParseRole(s.profile.Role).CanWrite() {
		return nil, fmt.Errorf("permission denied: profile role is read-only")
	}

	var args struct {
		Content   string `json:"content"`
		Kind      string `json:"kind"`
		Scope     string `json:"scope"`
		Metadata  string `json:"metadata"`
		SessionID string `json:"session_id"`
		Entities  string `json:"entities"`
		Working   bool   `json:"working"`
		Staged    *bool  `json:"staged"`
	}
	if err := json.Unmarshal(input, &args); err != nil {
		return nil, fmt.Errorf("invalid arguments for 'memory_set': failed to parse arguments: %w", err)
	}
	if args.Content == "" {
		return nil, fmt.Errorf("invalid arguments for 'memory_set': 'content' is required")
	}

	// Semantic kind (#486): required, snapped onto the canonical buckets.
	canonicalKind, ok := db.NormalizeKind(args.Kind)
	if !ok {
		return nil, fmt.Errorf("invalid arguments for 'memory_set': 'kind' is required and must be one of: %s (got %q)",
			strings.Join(db.ValidKinds(), ", "), args.Kind)
	}

	// Staging (#485): explicit per-call flag wins; otherwise the server
	// default (stage_writes_by_default) decides for low-trust clients.
	staged := false
	if args.Staged != nil {
		staged = *args.Staged
	} else if s.cfg != nil && s.cfg.Memory.StageWritesByDefault {
		staged = true
	}

	meta := make(map[string]string)
	if args.Metadata != "" {
		if err := json.Unmarshal([]byte(args.Metadata), &meta); err != nil {
			return nil, fmt.Errorf("invalid arguments for 'memory_set': 'metadata' must be a valid JSON object: %w", err)
		}
	}

	var entityNames []string
	if args.Entities != "" {
		for _, e := range strings.Split(args.Entities, ",") {
			e = strings.TrimSpace(e)
			if e != "" {
				entityNames = append(entityNames, e)
			}
		}
	}

	ttl := s.workingMemoryTTL
	actor := s.attributionActor()
	id, err := s.service.SetGoverned(args.Content, args.Scope, meta, args.SessionID, actor, entityNames, actor, args.Working, ttl, canonicalKind, staged)
	if err != nil {
		return nil, fmt.Errorf("%s", err.Error())
	}

	if staged {
		return fmt.Sprintf("Memory staged as candidate (not yet retrievable) with ID: %s. Review it with memory_candidates / memory_promote / memory_reject.", id), nil
	}
	return fmt.Sprintf("Memory saved successfully with ID: %s", id), nil
}

// handleMemoryCandidates lists the staged review queue (#485).
func (s *Server) handleMemoryCandidates(ctx context.Context, input json.RawMessage) (any, error) {
	var args struct {
		Limit int `json:"limit"`
	}
	if err := json.Unmarshal(input, &args); err != nil {
		return nil, fmt.Errorf("invalid arguments for 'memory_candidates': failed to parse arguments: %w", err)
	}
	if args.Limit < 1 {
		args.Limit = 100
	}
	candidates, err := s.service.Candidates(args.Limit)
	if err != nil {
		return mcpError("Failed to list staged candidates", err)
	}
	if len(candidates) == 0 {
		return "No staged candidates awaiting review.", nil
	}

	compact := make([]MemoryResponse, len(candidates))
	for i, c := range candidates {
		compact[i] = memoryResponse(c)
	}
	page := struct {
		Count   int              `json:"count"`
		Results []MemoryResponse `json:"results"`
	}{Count: len(compact), Results: compact}
	data, _ := json.MarshalIndent(page, "", "  ")
	return string(data), nil
}

// handleMemoryPromote approves a staged candidate (#485).
func (s *Server) handleMemoryPromote(ctx context.Context, input json.RawMessage) (any, error) {
	var args struct {
		ID string `json:"id"`
	}
	if err := json.Unmarshal(input, &args); err != nil {
		return nil, fmt.Errorf("invalid arguments for 'memory_promote': failed to parse arguments: %w", err)
	}
	if args.ID == "" {
		return nil, fmt.Errorf("invalid arguments for 'memory_promote': 'id' is required")
	}
	if err := s.service.Promote(args.ID); err != nil {
		if nf, ok := err.(*NotFoundError); ok {
			return nf.Error(), nil
		}
		return mcpError("Failed to promote memory", err)
	}
	return fmt.Sprintf("Memory %s promoted: it is now retrievable.", args.ID), nil
}

// handleMemoryReject discards a staged candidate (#485).
func (s *Server) handleMemoryReject(ctx context.Context, input json.RawMessage) (any, error) {
	var args struct {
		ID string `json:"id"`
	}
	if err := json.Unmarshal(input, &args); err != nil {
		return nil, fmt.Errorf("invalid arguments for 'memory_reject': failed to parse arguments: %w", err)
	}
	if args.ID == "" {
		return nil, fmt.Errorf("invalid arguments for 'memory_reject': 'id' is required")
	}
	if err := s.service.Reject(args.ID); err != nil {
		if nf, ok := err.(*NotFoundError); ok {
			return nf.Error(), nil
		}
		return mcpError("Failed to reject memory", err)
	}
	return fmt.Sprintf("Memory %s rejected and removed.", args.ID), nil
}

func (s *Server) handleMemorySearch(ctx context.Context, input json.RawMessage) (any, error) {
	var args struct {
		Query             string  `json:"query"`
		Scope             string  `json:"scope"`
		SessionID         string  `json:"session_id"`
		Profile           string  `json:"profile"`
		Limit             int     `json:"limit"`
		Entity            string  `json:"entity"`
		MinConfidence     string  `json:"min_confidence"`
		Verification      string  `json:"verification"`
		ExcludeSuperseded bool    `json:"exclude_superseded"`
		MaxAge            string  `json:"max_age"`
		MaxSensitivity    string  `json:"max_sensitivity"`
		MinSharingLevel   string  `json:"min_sharing_level"`
		ClientID          string  `json:"client_id"`
		WithEvidence      bool    `json:"with_evidence"`
		MinScore          float64 `json:"min_score"`
		MaxPayloadBytes   int     `json:"max_payload_bytes"`
		Cursor            string  `json:"cursor"`

		From string `json:"from"`
		To   string `json:"to"`
	}
	if err := json.Unmarshal(input, &args); err != nil {
		return nil, fmt.Errorf("invalid arguments for 'memory_search': failed to parse arguments: %w", err)
	}
	if args.Query == "" {
		return nil, fmt.Errorf("invalid arguments for 'memory_search': 'query' is required")
	}

	limit := args.Limit
	if limit <= 0 {
		limit = 5
	}

	// Log the search query
	truncatedParams := fmt.Sprintf(`{"query":%q,"scope":%q,"profile":%q,"limit":%d,"entity":%q}`,
		args.Query, args.Scope, args.Profile, limit, args.Entity)
	if len(truncatedParams) > 500 {
		truncatedParams = truncatedParams[:500] + "..."
	}
	startTime := time.Now()

	trustFilter := db.TrustFilter{
		MinConfidence:      args.MinConfidence,
		VerificationStatus: args.Verification,
		ExcludeSuperseded:  args.ExcludeSuperseded,
	}
	if args.MaxAge != "" {
		dur, err := parseDuration(args.MaxAge)
		if err != nil {
			return nil, fmt.Errorf("invalid arguments for 'memory_search': invalid max_age: %w", err)
		}
		trustFilter.MaxAge = dur
	}

	policyFilter := db.PolicyFilter{
		MaxSensitivity:  args.MaxSensitivity,
		MinSharingLevel: args.MinSharingLevel,
		ClientID:        args.ClientID,
	}

	// Build time window from explicit from/to.
	timeWindow := db.TimeWindow{}
	if args.From != "" {
		t, err := parseTimeArg(args.From)
		if err != nil {
			return nil, fmt.Errorf("invalid arguments for 'memory_search': invalid from: %w", err)
		}
		timeWindow.From = &t
	}
	if args.To != "" {
		t, err := parseTimeArg(args.To)
		if err != nil {
			return nil, fmt.Errorf("invalid arguments for 'memory_search': invalid to: %w", err)
		}
		timeWindow.To = &t
	}

	// Temporal extraction from query text (config-gated, off by default).
	if s.cfg != nil && s.cfg.Search.TemporalExtractionEnabled && timeWindow.From == nil && timeWindow.To == nil {
		if tw, err := extractTemporalFromQuery(args.Query); err == nil && tw != nil {
			timeWindow = *tw
		}
	}

	var searchResults []db.SearchResult
	var err error

	useHybrid := s.cfg != nil && s.cfg.HybridSearch.Enabled && args.Profile == ""

	if useHybrid {
		hybridResults, hErr := s.service.HybridSearch(
			args.Query, args.Scope, limit, args.Entity,
			trustFilter, policyFilter,
			s.cfg.HybridSearch.VectorWeight, s.cfg.HybridSearch.BM25Weight,
			timeWindow,
		)
		if hErr != nil {
			if nf, ok := hErr.(*NotFoundError); ok {
				return nil, fmt.Errorf("%s", nf.Error())
			}
			return mcpError("Failed to search memories", hErr)
		}

		minScore := args.MinScore
		if minScore <= 0 && s.cfg != nil {
			minScore = s.cfg.Search.MinScore
		}
		if minScore > 0 {
			before := len(hybridResults)
			hybridResults = db.FilterHybridByMinScore(hybridResults, minScore)
			if len(hybridResults) == 0 && before > 0 {
				return fmt.Sprintf("No confident match: all %d hybrid result(s) scored below the min_score threshold %.3f.", before, minScore), nil
			}
		}

		searchResults = hybridResultsToSearchResults(hybridResults)
	} else if args.Profile != "" {
		searchResults, err = s.service.SearchWithProfile(args.Query, args.Profile, limit, args.Entity, trustFilter, policyFilter, timeWindow)
	} else {
		searchResults, err = s.service.Search(args.Query, args.Scope, limit, args.Entity, trustFilter, policyFilter, timeWindow)
	}

	if err != nil {
		if nf, ok := err.(*NotFoundError); ok {
			return nil, fmt.Errorf("%s", nf.Error())
		}
		return mcpError("Failed to search memories", err)
	}

	// Apply cursor-based pagination: filter to results before the cursor time
	if args.Cursor != "" {
		cursorTime, err := parseCursor(args.Cursor)
		if err != nil {
			return nil, fmt.Errorf("invalid arguments for 'memory_search': invalid cursor: %w", err)
		}
		var filtered []db.SearchResult
		for _, r := range searchResults {
			if r.Memory != nil && r.Memory.CreatedAt.Before(cursorTime) {
				filtered = append(filtered, r)
			}
		}
		searchResults = filtered
	}

	// Apply min-score filtering (only for non-hybrid path; hybrid handled above)
	if !useHybrid {
		minScore := args.MinScore
		if minScore <= 0 && s.cfg != nil {
			minScore = s.cfg.Search.MinScore
		}
		if minScore > 0 {
			before := len(searchResults)
			searchResults = db.FilterByMinScore(searchResults, minScore)
			if len(searchResults) == 0 && before > 0 {
				_, _ = s.service.LogQuery(s.attributionActor(), args.Scope, args.SessionID, "memory_search", args.Query, truncatedParams, time.Since(startTime).Milliseconds())
				return fmt.Sprintf("No confident match: all %d result(s) scored below the min_score threshold %.3f.", before, minScore), nil
			}
		}
	}

	if len(searchResults) == 0 {
		_, _ = s.service.LogQuery(s.attributionActor(), args.Scope, args.SessionID, "memory_search", args.Query, truncatedParams, time.Since(startTime).Milliseconds())
		return "No relevant memories found.", nil
	}

	compact := make([]SearchResultResponse, len(searchResults))
	for i, r := range searchResults {
		compact[i] = searchResultResponse(r)
		if s.cfg != nil && s.cfg.MCP.RecallReceipts {
			compact[i].Receipt = memory.Receipt(r.Memory, time.Now())
		}
		if args.WithEvidence && r.Memory != nil {
			evidence, err := s.service.GetMemoryEvidence(r.Memory.ID)
			if err == nil {
				compact[i].Memory.Evidence = evidence
			}
		}
	}

	// Apply payload size cap
	if args.MaxPayloadBytes > 0 {
		compact = truncatePayloadByBytes(compact, args.MaxPayloadBytes)
	}

	// Build next cursor
	pageInfo := struct {
		Results    []SearchResultResponse `json:"results"`
		NextCursor string                 `json:"next_cursor,omitempty"`
		Truncated  bool                   `json:"truncated,omitempty"`
	}{
		Results: compact,
	}

	// Generate next cursor from the last result if there are results
	if len(compact) > 0 && compact[len(compact)-1].Memory.CreatedAt != (time.Time{}) {
		pageInfo.NextCursor = compact[len(compact)-1].Memory.CreatedAt.UTC().Format(time.RFC3339Nano)
		pageInfo.Truncated = len(compact) < len(searchResults) || (args.MaxPayloadBytes > 0 && len(compact) != len(searchResults))
	}

	queryID, err := s.service.LogQuery(s.attributionActor(), args.Scope, args.SessionID, "memory_search", args.Query, truncatedParams, time.Since(startTime).Milliseconds())
	if err == nil && queryID != "" {
		refs := make([]db.QueryResultRef, len(searchResults))
		for i, r := range searchResults {
			refs[i] = db.QueryResultRef{MemoryID: r.Memory.ID, Rank: i, Score: float64(r.Score)}
		}
		// Best-effort telemetry: a recording failure never fails the search.
		_ = s.service.RecordQueryResults(queryID, refs)
	}

	data, _ := json.MarshalIndent(pageInfo, "", "  ")
	return string(data), nil
}

func (s *Server) handleMemoryList(ctx context.Context, input json.RawMessage) (any, error) {
	var args struct {
		Scope           string `json:"scope"`
		Limit           int    `json:"limit"`
		MaxSensitivity  string `json:"max_sensitivity"`
		MinSharingLevel string `json:"min_sharing_level"`
		ClientID        string `json:"client_id"`
		AsOf            string `json:"as_of"`
		MaxPayloadBytes int    `json:"max_payload_bytes"`
		Cursor          string `json:"cursor"`
	}
	if err := json.Unmarshal(input, &args); err != nil {
		return nil, fmt.Errorf("invalid arguments for 'memory_list': failed to parse arguments: %w", err)
	}

	limit := args.Limit
	if limit < 1 {
		limit = 100
	}
	if limit > 1000 {
		limit = 1000
	}

	startTime := time.Now()

	if args.AsOf != "" {
		asOf, err := time.Parse(time.RFC3339, args.AsOf)
		if err != nil {
			return nil, fmt.Errorf("invalid arguments for 'memory_list': 'as_of' must be RFC3339: %w", err)
		}
		memories, err := s.service.ListMemoriesAsOf(args.Scope, asOf, limit)
		if err != nil {
			return mcpError("Failed to list memories as of the given time", err)
		}
		if len(memories) == 0 {
			_, _ = s.service.LogQuery(s.attributionActor(), args.Scope, "", "memory_list", "as_of", args.AsOf, time.Since(startTime).Milliseconds())
			return "No memories were valid at that point in time.", nil
		}

		// Apply cursor-based pagination
		if args.Cursor != "" {
			cursorTime, err := parseCursor(args.Cursor)
			if err != nil {
				return nil, fmt.Errorf("invalid arguments for 'memory_list': invalid cursor: %w", err)
			}
			var filtered []*db.Memory
			for _, m := range memories {
				if m.CreatedAt.Before(cursorTime) {
					filtered = append(filtered, m)
				}
			}
			memories = filtered
		}

		// Apply payload size cap
		if args.MaxPayloadBytes > 0 {
			memories = truncatePayloadByBytes(memories, args.MaxPayloadBytes)
		}

		pageInfo := buildMemoryListPage(memories, len(memories) < limit)
		_, _ = s.service.LogQuery(s.attributionActor(), args.Scope, "", "memory_list", "as_of", args.AsOf, time.Since(startTime).Milliseconds())
		data, _ := json.MarshalIndent(pageInfo, "", "  ")
		return string(data), nil
	}

	policyFilter := db.PolicyFilter{
		MaxSensitivity:  args.MaxSensitivity,
		MinSharingLevel: args.MinSharingLevel,
		ClientID:        args.ClientID,
	}

	var queryText string
	memories, err := s.service.ListWithPolicy(args.Scope, limit, policyFilter)
	if err != nil {
		return mcpError("Failed to list memories", err)
	}

	if args.Cursor != "" {
		cursorTime, err := parseCursor(args.Cursor)
		if err != nil {
			return nil, fmt.Errorf("invalid arguments for 'memory_list': invalid cursor: %w", err)
		}
		var filtered []*db.Memory
		for _, m := range memories {
			if m.CreatedAt.Before(cursorTime) {
				filtered = append(filtered, m)
			}
		}
		memories = filtered
	}

	if len(memories) == 0 {
		_, _ = s.service.LogQuery(s.attributionActor(), args.Scope, "", "memory_list", queryText, args.Scope, time.Since(startTime).Milliseconds())
		return "Memory store is empty.", nil
	}

	// Apply payload size cap
	if args.MaxPayloadBytes > 0 {
		memories = truncatePayloadByBytes(memories, args.MaxPayloadBytes)
	}

	pageInfo := buildMemoryListPage(memories, len(memories) < limit)
	_, _ = s.service.LogQuery(s.attributionActor(), args.Scope, "", "memory_list", queryText, args.Scope, time.Since(startTime).Milliseconds())

	data, _ := json.MarshalIndent(pageInfo, "", "  ")
	return string(data), nil
}

func (s *Server) handleEntityList(ctx context.Context, input json.RawMessage) (any, error) {
	entities, err := s.service.ListEntities()
	if err != nil {
		return mcpError("Failed to list entities", err)
	}

	if len(entities) == 0 {
		return "No entities found.", nil
	}

	data, _ := json.MarshalIndent(entities, "", "  ")
	return string(data), nil
}

func (s *Server) handleEntityResolve(ctx context.Context, input json.RawMessage) (any, error) {
	var args struct {
		Query   string `json:"query"`
		Type    string `json:"type"`
		Aliases string `json:"aliases"`
		Limit   int    `json:"limit"`
	}
	if err := json.Unmarshal(input, &args); err != nil {
		return nil, fmt.Errorf("invalid arguments for 'entity_resolve': failed to parse arguments: %w", err)
	}
	if args.Query == "" {
		return nil, fmt.Errorf("invalid arguments for 'entity_resolve': 'query' is required")
	}
	if args.Limit == 0 {
		args.Limit = 10
	}

	var aliasHints []string
	if args.Aliases != "" {
		for _, a := range strings.Split(args.Aliases, ",") {
			a = strings.TrimSpace(a)
			if a != "" {
				aliasHints = append(aliasHints, a)
			}
		}
	}

	candidates, err := s.service.ResolveEntityCandidates(args.Query, args.Type, aliasHints, args.Limit)
	if err != nil {
		return mcpError("Failed to resolve entity candidates", err)
	}
	if len(candidates) == 0 {
		return "No matching entities found.", nil
	}

	data, _ := json.MarshalIndent(candidates, "", "  ")
	return string(data), nil
}

func (s *Server) handleEntityRelate(ctx context.Context, input json.RawMessage) (any, error) {
	var args struct {
		From         string `json:"from"`
		To           string `json:"to"`
		FromID       string `json:"from_id"`
		ToID         string `json:"to_id"`
		Relation     string `json:"relation"`
		Action       string `json:"action"`
		Source       string `json:"source"`
		SourceRef    string `json:"source_ref"`
		Verification string `json:"verification"`
		Evidence     string `json:"evidence"`
		ValidFrom    string `json:"valid_from"`
		ValidUntil   string `json:"valid_until"`
	}
	if err := json.Unmarshal(input, &args); err != nil {
		return nil, fmt.Errorf("invalid arguments for 'entity_relate': failed to parse arguments: %w", err)
	}
	if args.Relation == "" {
		return nil, fmt.Errorf("invalid arguments for 'entity_relate': 'relation' is required")
	}
	if args.Action == "" {
		args.Action = "create"
	}

	hasIDs := args.FromID != "" || args.ToID != ""
	hasNames := args.From != "" || args.To != ""
	if hasIDs && hasNames {
		return nil, fmt.Errorf("invalid arguments for 'entity_relate': provide either 'from'/'to' names or 'from_id'/'to_id', not both")
	}

	var fromEntity, toEntity *db.Entity
	var err error

	if hasIDs {
		if args.FromID == "" || args.ToID == "" {
			return nil, fmt.Errorf("invalid arguments for 'entity_relate': 'from_id' and 'to_id' must both be set")
		}
		fromEntity, err = s.service.GetEntityByID(args.FromID)
		if err != nil {
			return mcpError("Failed to fetch source entity", err)
		}
		if fromEntity == nil {
			return nil, fmt.Errorf("entity not found: %s", args.FromID)
		}
		toEntity, err = s.service.GetEntityByID(args.ToID)
		if err != nil {
			return mcpError("Failed to fetch target entity", err)
		}
		if toEntity == nil {
			return nil, fmt.Errorf("entity not found: %s", args.ToID)
		}
	} else {
		if args.From == "" || args.To == "" {
			return nil, fmt.Errorf("invalid arguments for 'entity_relate': 'from' and 'to' are required")
		}
		fromEntity, err = s.service.ResolveEntity(args.From)
		if err != nil {
			return mcpError("Failed to resolve source entity", err)
		}
		if fromEntity == nil {
			return nil, fmt.Errorf("entity not found: %s", args.From)
		}
		toEntity, err = s.service.ResolveEntity(args.To)
		if err != nil {
			return mcpError("Failed to resolve target entity", err)
		}
		if toEntity == nil {
			return nil, fmt.Errorf("entity not found: %s", args.To)
		}
	}

	hasProvenance := args.Source != "" || args.SourceRef != "" || args.Verification != "" || args.Evidence != ""
	if hasProvenance && (args.Source == "") != (args.SourceRef == "") {
		return nil, fmt.Errorf("invalid arguments for 'entity_relate': 'source' and 'source_ref' must be provided together")
	}

	switch args.Action {
	case "create":
		var validFrom, validUntil *time.Time
		if args.ValidFrom != "" {
			t, err := db.ParseRelationDate(args.ValidFrom)
			if err != nil {
				return nil, fmt.Errorf("invalid arguments for 'entity_relate': invalid valid_from: %w", err)
			}
			validFrom = &t
		}
		if args.ValidUntil != "" {
			t, err := db.ParseRelationDate(args.ValidUntil)
			if err != nil {
				return nil, fmt.Errorf("invalid arguments for 'entity_relate': invalid valid_until: %w", err)
			}
			validUntil = &t
		}

		if hasIDs || hasProvenance || validFrom != nil {
			rel := &db.EntityRelation{
				FromEntityID: fromEntity.ID,
				ToEntityID:   toEntity.ID,
				RelationType: args.Relation,
				Source:       args.Source,
				SourceRef:    args.SourceRef,
				Verification: args.Verification,
				Evidence:     args.Evidence,
				CreatedBy:    s.attributionActor(),
				ValidFrom:    validFrom,
				ValidUntil:   validUntil,
			}
			saved, err := s.service.SaveEntityRelationProvenance(rel)
			if err != nil {
				var conflict *db.VerifiedRelationConflictError
				if errors.As(err, &conflict) {
					return nil, fmt.Errorf("invalid arguments for 'entity_relate': %w", err)
				}
				return mcpError("Failed to save relation", err)
			}
			data, _ := json.MarshalIndent(saved, "", "  ")
			return string(data), nil
		}

		rel := &db.EntityRelation{
			FromEntityID: fromEntity.ID,
			ToEntityID:   toEntity.ID,
			RelationType: args.Relation,
			CreatedBy:    s.attributionActor(),
			CreatedAt:    time.Now().UTC(),
			ValidFrom:    validFrom,
			ValidUntil:   validUntil,
		}
		if err := s.service.SaveEntityRelation(rel); err != nil {
			return mcpError("Failed to save relation", err)
		}
		return fmt.Sprintf("Related: %s --%s--> %s", fromEntity.Name, args.Relation, toEntity.Name), nil
	case "delete":
		if err := s.service.DeleteEntityRelation(fromEntity.ID, toEntity.ID, args.Relation); err != nil {
			return mcpError("Failed to delete relation", err)
		}
		return fmt.Sprintf("Unrelated: %s --%s--> %s", fromEntity.Name, args.Relation, toEntity.Name), nil
	default:
		return nil, fmt.Errorf("invalid arguments for 'entity_relate': 'action' must be 'create' or 'delete', got %q", args.Action)
	}
}

func (s *Server) handleGraphNeighbors(ctx context.Context, input json.RawMessage) (any, error) {
	var args struct {
		Entity string `json:"entity"`
		Depth  int    `json:"depth"`
		AsOf   string `json:"as_of"`
	}
	if err := json.Unmarshal(input, &args); err != nil {
		return nil, fmt.Errorf("invalid arguments for 'graph_neighbors': failed to parse arguments: %w", err)
	}
	if args.Entity == "" {
		return nil, fmt.Errorf("invalid arguments for 'graph_neighbors': 'entity' is required")
	}
	if args.Depth == 0 {
		args.Depth = 1
	}

	entity, err := s.service.ResolveEntity(args.Entity)
	if err != nil {
		return mcpError("Failed to resolve entity", err)
	}
	if entity == nil {
		return nil, fmt.Errorf("entity not found: %s", args.Entity)
	}

	var asOf *time.Time
	if args.AsOf != "" {
		t, err := db.ParseRelationDate(args.AsOf)
		if err != nil {
			return nil, fmt.Errorf("invalid arguments for 'graph_neighbors': invalid as_of: %w", err)
		}
		asOf = &t
	}

	var nodes []*db.Entity
	var edges []*db.EntityRelation
	if asOf != nil {
		nodes, edges, err = s.service.GraphNeighborsAsOf(entity.ID, args.Depth, asOf)
	} else {
		nodes, edges, err = s.service.GraphNeighbors(entity.ID, args.Depth)
	}
	if err != nil {
		return nil, fmt.Errorf("invalid arguments for 'graph_neighbors': %w", err)
	}

	data, _ := json.MarshalIndent(struct {
		Nodes []*db.Entity         `json:"nodes"`
		Edges []*db.EntityRelation `json:"edges"`
	}{Nodes: nodes, Edges: edges}, "", "  ")
	return string(data), nil
}

func parseDuration(s string) (time.Duration, error) {
	if s == "" {
		return 0, nil
	}
	if len(s) < 2 {
		return 0, fmt.Errorf("invalid duration: %s", s)
	}
	suffix := s[len(s)-1]
	switch suffix {
	case 'd':
		n, err := strconv.Atoi(s[:len(s)-1])
		if err != nil {
			return 0, err
		}
		return time.Duration(n) * 24 * time.Hour, nil
	case 'h':
		return time.ParseDuration(s)
	case 'm':
		return time.ParseDuration(s)
	case 's':
		return time.ParseDuration(s)
	default:
		return time.ParseDuration(s)
	}
}

// handleQueryLog returns the query log summary with tool and actor
// breakdowns and recent entries, optionally narrowed to one actor.
func (s *Server) handleQueryLog(ctx context.Context, input json.RawMessage) (any, error) {
	var args struct {
		Limit int    `json:"limit"`
		Actor string `json:"actor"`
	}
	if err := json.Unmarshal(input, &args); err != nil {
		return nil, fmt.Errorf("invalid arguments for 'query_log': failed to parse arguments: %w", err)
	}
	if args.Limit <= 0 {
		args.Limit = 20
	}
	if args.Limit > 100 {
		args.Limit = 100
	}

	summary, err := s.service.GetQueryLogSummary(args.Limit, args.Actor)
	if err != nil {
		return mcpError("Failed to retrieve query log", err)
	}

	data, _ := json.MarshalIndent(summary, "", "  ")
	return string(data), nil
}

// truncatePayloadByBytes truncates a results slice so the marshalled JSON
// stays within maxBytes. Returns the trimmed slice. When maxBytes is 0,
// no truncation is applied.
func truncatePayloadByBytes[T any](results []T, maxBytes int) []T {
	if maxBytes <= 0 || len(results) == 0 {
		return results
	}
	// Start with all results, then binary-search for the safe count.
	// Serialize incrementally to avoid quadratic cost on large sets.
	for i := len(results); i > 0; i-- {
		data, err := json.MarshalIndent(results[:i], "", "  ")
		if err == nil && len(data) <= maxBytes {
			return results[:i]
		}
	}
	return nil
}

// extractCursorFromResults generates a cursor string from a result slice
// for cursor-based pagination. Uses the last result's creation timestamp.
//
//nolint:unused // utility, available when list handlers adopt cursors
func extractCursorFromResults[T interface{ GetCreatedAt() time.Time }](results []T) string {
	if len(results) == 0 {
		return ""
	}
	return results[len(results)-1].GetCreatedAt().UTC().Format(time.RFC3339Nano)
}

// parseCursor parses a cursor string into a time.Time for pagination.
func parseCursor(cursor string) (time.Time, error) {
	if cursor == "" {
		return time.Time{}, nil
	}
	return time.Parse(time.RFC3339Nano, cursor)
}

// memoryListPage is the paginated response for memory_list.
type memoryListPage struct {
	Memories   []*db.Memory `json:"memories"`
	NextCursor string       `json:"next_cursor,omitempty"`
	Truncated  bool         `json:"truncated,omitempty"`
}

// buildMemoryListPage wraps a memory slice into a paginated response.
func buildMemoryListPage(memories []*db.Memory, atEnd bool) memoryListPage {
	page := memoryListPage{Memories: memories}
	if len(memories) > 0 && !atEnd {
		page.NextCursor = memories[len(memories)-1].CreatedAt.UTC().Format(time.RFC3339Nano)
	}
	page.Truncated = !atEnd
	return page
}

// parseTimeArg parses an RFC3339 or YYYY-MM-DD timestamp string.
func parseTimeArg(s string) (time.Time, error) {
	// Try RFC3339 first.
	if t, err := time.Parse(time.RFC3339, s); err == nil {
		return t.UTC(), nil
	}
	// Try YYYY-MM-DD.
	if t, err := time.Parse("2006-01-02", s); err == nil {
		return t.UTC(), nil
	}
	// Try YYYY-MM-DDTHH:MM:SS without timezone (treat as UTC).
	if t, err := time.Parse("2006-01-02T15:04:05", s); err == nil {
		return t.UTC(), nil
	}
	return time.Time{}, fmt.Errorf("cannot parse %q as RFC3339 or YYYY-MM-DD", s)
}

// extractTemporalFromQuery runs the query through the temporal extractor and
// returns a *db.TimeWindow when a match is found (nil when no expression).
func extractTemporalFromQuery(query string) (*db.TimeWindow, error) {
	result := temporal.Extract(query, time.Now())
	if result == nil {
		return nil, nil
	}
	tw := &db.TimeWindow{}
	if result.Window.From != nil {
		tw.From = result.Window.From
	}
	if result.Window.To != nil {
		tw.To = result.Window.To
	}
	return tw, nil
}
