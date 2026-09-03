package gateway

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/danieljustus/symaira-brain/internal/audit"
	"github.com/danieljustus/symaira-brain/internal/broker"
	"github.com/danieljustus/symaira-brain/internal/catalog"
	"github.com/danieljustus/symaira-brain/internal/config"
	memorymcp "github.com/danieljustus/symaira-brain/internal/memory/mcp"
	"github.com/danieljustus/symaira-brain/internal/patterns"
	"github.com/danieljustus/symaira-brain/internal/policy"
	"github.com/danieljustus/symaira-brain/internal/profile"
	"github.com/danieljustus/symaira-brain/internal/skills/mcptools"
	"github.com/danieljustus/symaira-brain/internal/usage"
	"github.com/danieljustus/symaira-brain/internal/xdg"
	"github.com/danieljustus/symaira-corekit/mcpserver"
)

// Server is the MCP gateway: it presents a merged, policy-filtered tool
// catalog to the harness and routes tools/call requests to the owning
// child server by stripping the namespace prefix.
type Server struct {
	profile           *profile.Profile
	servers           map[string]*broker.ManagedServer
	memoryServer      *memorymcp.Server
	cat               *catalog.Catalog
	logger            *slog.Logger
	cfg               *config.Config
	version           string
	degradations      []audit.Degradation
	auditDegradedWarn sync.Once
}

// New creates a gateway Server from a profile, pre-built managed
// servers, the global config, and a version string (typically set at
// build time via ldflags). The catalog is built lazily on the first
// ServeIO call (since tools/list requires live child connections).
func New(p *profile.Profile, servers map[string]*broker.ManagedServer, logger *slog.Logger, cfg *config.Config, version string) *Server {
	if logger == nil {
		logger = slog.Default()
	}
	return &Server{
		profile: p,
		servers: servers,
		logger:  logger,
		cfg:     cfg,
		version: version,
	}
}

// SetMemoryServer attaches the embedded symmemory MCP server (nil when the
// memory core is unavailable or disabled). Kept as a setter rather than a
// constructor argument so the many gateway test call sites stay unchanged.
func (s *Server) SetMemoryServer(ms *memorymcp.Server) {
	s.memoryServer = ms
}

// inProcessHandler is a type alias for the mcpserver.Tool.Handler
// signature. It exists so wrapHandler and its callers can name the type
// without depending on a named type from mcpserver (which has none) or
// forcing every caller to spell the function signature out in full.
type inProcessHandler = func(ctx context.Context, input json.RawMessage) (any, error)

// auditSink is the subset of the audit.Logger API that ServeIO uses. It
// is an interface so tests can substitute a fake logger (e.g. to force
// the degraded-warning path) without changing production behavior.
type auditSink interface {
	Log(server, tool string, args json.RawMessage, duration time.Duration, status string, exposure audit.Exposure, classifications ...audit.Classification)
	LogDegradation(server, reason, level string)
	Degraded() bool
	Close() error
}

// auditOpen opens the profile's audit logger. It is a package variable
// (test seam) so tests can inject a fake sink or force an open failure;
// production code never reassigns it.
var auditOpen = func(name string, cfg audit.Config) (auditSink, error) {
	return audit.Open(name, cfg)
}

// ServeIO serves the MCP protocol over the given reader/writer pair
// (stdin/stdout). It blocks until the client disconnects or ctx is
// cancelled.
func (s *Server) ServeIO(ctx context.Context, r io.Reader, w io.Writer) error {
	if err := s.buildCatalog(ctx); err != nil {
		return fmt.Errorf("gateway: build catalog: %w", err)
	}

	var auditLog auditSink
	auditEnabled := s.profile.Audit.Enabled
	if s.cfg != nil {
		auditEnabled = auditEnabled || s.cfg.Audit.Enabled
	}
	if auditEnabled {
		verb := false
		if s.cfg != nil {
			verb = s.cfg.Audit.Verbose
		}
		cfg := audit.Config{
			Enabled: true,
			Verbose: verb,
		}
		al, err := auditOpen(s.profile.Name, cfg)
		if err != nil {
			s.logger.Warn("failed to open audit log", "error", err)
		} else {
			auditLog = al
			defer auditLog.Close()
			for _, degradation := range s.degradations {
				auditLog.LogDegradation(degradation.Server, degradation.Reason, degradation.Level)
			}
		}
	}

	// Episode recording captures this connection's tool-call sequence
	// (names only) and flushes it as one episode when the session ends.
	var recorder *episodeRecorder
	if s.patternsEnabled() {
		recorder = newEpisodeRecorder(s.profile.Name)
		defer s.flushEpisode(recorder)
	}

	srv := mcpserver.New("symbrain", s.version)
	srv.SetInstructions(s.instructions())

	// The gateway-owned bootstrap tool is registered alongside the
	// forwarded child tools. It is never filtered by policy: it is
	// symbrain's own orientation surface, not a child capability.
	srv.RegisterTool(&mcpserver.Tool{
		Name:        "bootstrap",
		Description: bootstrapToolDescription,
		InputSchema: json.RawMessage(`{"type":"object","properties":{}}`),
		Annotations: &mcpserver.ToolAnnotations{Title: "Bootstrap", ReadOnlyHint: true, IdempotentHint: true},
		Handler:     s.handleBootstrap,
	})

	// The patterns tool exposes promoted, recurring tool sequences as
	// read-only context. Like bootstrap, it is gateway-owned and never
	// policy-filtered.
	srv.RegisterTool(&mcpserver.Tool{
		Name:        "patterns",
		Description: patternsToolDescription,
		InputSchema: json.RawMessage(`{"type":"object","properties":{}}`),
		Annotations: &mcpserver.ToolAnnotations{Title: "Patterns", ReadOnlyHint: true, IdempotentHint: true},
		Handler:     s.handlePatterns,
	})

	// Skills are absorbed directly (repo consolidation step 4, phase 2):
	// register symskills tools on the gateway server instead of spawning a
	// symskills child process. Skills policy is enable/disable only (no mode
	// preset), so a single Enabled gate reproduces the previous catalog
	// filtering. Tools are gateway-owned like bootstrap/patterns and are not
	// routed through routeToolCall, but each individual tool handler is
	// still wrapped through wrapInProcess so calls are audited (#422).
	if s.profile.Server(profile.ServerSkills).Enabled {
		mcptools.Register(srv, mcptools.Options{
			Version: s.version,
			Wrap:    s.wrapInProcess(auditLog, recorder, "skills"),
		})
	}

	// Usage is gateway-owned like bootstrap/patterns: the single
	// get_ai_usage tool reports AI subscription/token usage from
	// internal/usage (issue #290). It is profile-gated via the usage
	// server's enabled flag (no mode preset, like skills).
	if s.profile.Server(profile.ServerUsage).Enabled {
		srv.RegisterTool(&mcpserver.Tool{
			Name:        "get_ai_usage",
			Description: getAIUsageToolDescription,
			InputSchema: json.RawMessage(`{"type":"object","properties":{}}`),
			Annotations: &mcpserver.ToolAnnotations{Title: "AI Usage", ReadOnlyHint: true, IdempotentHint: true},
			Handler:     s.handleAIUsage,
		})
	}

	// Memory is absorbed directly (repo consolidation step 4, phase 2b): the
	// embedded memory server registers its tools in-process instead of a
	// spawned symmemory child. Its own JWT/profile attribution is
	// preconfigured by the caller; here we expose only the tools the profile
	// policy allows (mode preset + tools_allow/tools_deny). Each tool
	// handler is wrapped through wrapInProcess so calls are audited (#422).
	if s.memoryServer != nil && s.profile.Server(profile.ServerMemory).Enabled {
		report, err := policy.EvaluatePreset(profile.ServerMemory, s.profile.Server(profile.ServerMemory))
		if err != nil {
			s.logger.Warn("failed to evaluate memory policy", "error", err)
		} else {
			allowed := make(map[string]bool, len(report.Exposed))
			for _, name := range report.Exposed {
				allowed[name] = true
			}
			s.memoryServer.RegisterTools(srv, allowed, s.wrapInProcess(auditLog, recorder, "memory"))
		}
	}

	for _, entry := range s.cat.Exposed() {
		entry := entry
		exposure := audit.Exposure{AccessClass: entry.AccessClass, AccessSource: entry.AccessSource}
		srv.RegisterTool(&mcpserver.Tool{
			Name:        entry.Name,
			Description: entry.Description,
			InputSchema: entry.InputSchema,
			Annotations: catalogEntryAnnotations(entry),
			Handler: s.wrapHandler(auditLog, recorder, entry.Server, entry.OriginalName, exposure,
				func(ctx context.Context, input json.RawMessage) (any, error) {
					return s.routeToolCall(ctx, entry, input)
				}),
		})
	}

	return srv.ServeIO(ctx, r, w)
}

// wrapHandler builds the audit+episode closure applied around every tool
// call the gateway serves — both child-server tools routed through the
// catalog (routeToolCall) and gateway-owned in-process tools (memory,
// skills) registered directly on srv. It records the call in the episode
// recorder (names only) and, when auditLog is non-nil, emits one JSONL
// entry per call carrying status, duration, exposure, and the resolved
// error classification. auditLog and recorder may be nil (audit disabled /
// patterns disabled respectively), in which case the corresponding
// recording step is skipped.
func (s *Server) wrapHandler(auditLog auditSink, recorder *episodeRecorder, server, tool string, exposure audit.Exposure, h inProcessHandler) inProcessHandler {
	return func(ctx context.Context, input json.RawMessage) (any, error) {
		start := time.Now()
		result, err := h(ctx, input)
		if recorder != nil {
			recorder.Add(server, tool)
		}
		if auditLog != nil {
			status := "ok"
			var classification audit.Classification
			if err != nil {
				status = "error"
				if classified, ok := err.(*classifiedError); ok {
					classification = classified.Classification
				}
			}
			auditLog.Log(server, tool, input, time.Since(start), status, exposure, classification)
			if auditLog.Degraded() {
				s.auditDegradedWarn.Do(func() {
					s.logger.Warn("audit log degraded; some entries may not be persisted")
				})
			}
		}
		return result, err
	}
}

// wrapInProcess partially applies wrapHandler for one gateway-owned
// in-process tool server (memory or skills): every tool handler wrapped
// through the returned function is audited under that fixed server name.
// These tools have no catalog.Entry (they bypass routeToolCall entirely),
// so their audit entries carry a zero-value exposure — the same as any
// other core-alias catalog entry (see catalog.Build), since AccessClass/
// AccessSource only ever apply to foreign (non-core) servers.
func (s *Server) wrapInProcess(auditLog auditSink, recorder *episodeRecorder, server string) func(tool string, h inProcessHandler) inProcessHandler {
	return func(tool string, h inProcessHandler) inProcessHandler {
		return s.wrapHandler(auditLog, recorder, server, tool, audit.Exposure{}, h)
	}
}

// instructions returns the stable profile guidance and, when startup was
// degraded, a deterministic one-line summary of the absent backends.
func (s *Server) instructions() string {
	base := fmt.Sprintf("symbrain profile %q", s.profile.Name)
	if len(s.degradations) == 0 {
		return base
	}

	servers := make([]string, 0, len(s.degradations))
	for _, degradation := range s.degradations {
		servers = append(servers, degradation.Server)
	}
	sort.Strings(servers)
	return fmt.Sprintf("%s; degraded backends: %s", base, strings.Join(servers, ", "))
}

// buildCatalog queries each managed server for its tools, evaluates the
// policy, and builds the merged catalog. It must be called before
// registering tools with mcpserver.
func (s *Server) buildCatalog(ctx context.Context) error {
	var servers []catalog.ServerTools
	s.degradations = nil

	for alias, ms := range s.servers {
		serverCfg := s.profile.Server(alias)
		if !serverCfg.Enabled {
			continue
		}

		tools, err := ms.ListTools(ctx)
		if err != nil {
			s.logger.Warn("failed to list tools from child",
				"server", alias, "error", err)
			s.degradations = append(s.degradations, audit.Degradation{
				Server: alias,
				Reason: err.Error(),
				Level:  "warning",
			})
			continue
		}

		brokerTools := make([]catalog.Tool, len(tools))
		for i, t := range tools {
			brokerTools[i] = catalog.Tool{
				Name:        t.Name,
				Description: t.Description,
				InputSchema: t.InputSchema,
				Annotations: translateAnnotations(t.Annotations),
			}
		}

		liveNames := make([]string, len(tools))
		for i, t := range tools {
			liveNames[i] = t.Name
		}

		var report *policy.Report
		if profile.IsCoreAlias(alias) {
			report, err = policy.Evaluate(alias, serverCfg, liveNames)
		} else {
			// Foreign server: filter model — read/write access class plus
			// allow/deny, driven by the upstream readOnlyHint annotation.
			foreignTools := make([]policy.ForeignTool, len(tools))
			for i, t := range tools {
				foreignTools[i] = policy.ForeignTool{
					Name:         t.Name,
					ReadOnlyHint: readOnlyHint(t.Annotations),
				}
			}
			report, err = policy.EvaluateForeign(alias, serverCfg, foreignTools)
		}
		if err != nil {
			return fmt.Errorf("gateway: evaluate policy for %s: %w", alias, err)
		}

		// A foreign server under access=read that ends up exposing nothing
		// looks identical to a broken profile: most upstream servers don't
		// set readOnlyHint, so every tool falls through to default_write
		// and access=read hides all of them (see internal/policy/foreign.go
		// and symaira-corekit/mcpserver.go's doc comment on the annotation
		// gap). Say why instead of presenting a silent empty tool list.
		if !profile.IsCoreAlias(alias) && serverCfg.Access == profile.ForeignAccessRead &&
			len(tools) > 0 && len(report.Exposed) == 0 {
			s.degradations = append(s.degradations, audit.Degradation{
				Server: alias,
				Reason: fmt.Sprintf(
					"access=read exposes 0 of %d tools: none is classified as reading (no readOnlyHint from the upstream server and no tools_read override) — add tools_read entries for the tools that should be exposed",
					len(tools)),
				Level: "warning",
			})
		}

		servers = append(servers, catalog.ServerTools{
			Server: alias,
			Tools:  brokerTools,
			Report: report,
		})
	}

	cat, err := catalog.Build(servers)
	if err != nil {
		return err
	}
	s.cat = cat
	return nil
}

// catalogEntryAnnotations converts the catalog mirror into the corekit
// annotation type used by the gateway's tools/list response. A fallback
// annotation is always returned for upstream tools that omitted annotations;
// the explicit false Go value is intentionally retained even though corekit's
// current wire encoder omits false readOnlyHint values.
func catalogEntryAnnotations(entry catalog.Entry) *mcpserver.ToolAnnotations {
	annotations := &mcpserver.ToolAnnotations{Title: entry.Name}
	if entry.Annotations == nil {
		return annotations
	}
	if entry.Annotations.ReadOnlyHint != nil {
		annotations.ReadOnlyHint = *entry.Annotations.ReadOnlyHint
	}
	if entry.Annotations.DestructiveHint != nil {
		annotations.DestructiveHint = *entry.Annotations.DestructiveHint
	}
	if entry.Annotations.IDempotentHint != nil {
		annotations.IdempotentHint = *entry.Annotations.IDempotentHint
	}
	if entry.Annotations.OpenWorldHint != nil {
		annotations.OpenWorldHint = *entry.Annotations.OpenWorldHint
	}
	return annotations
}

// translateAnnotations copies broker.ToolAnnotations into the catalog's
// mirror type (same shape, separate package to avoid an import cycle).
func translateAnnotations(a *broker.ToolAnnotations) *catalog.ToolAnnotations {
	if a == nil {
		return nil
	}
	return &catalog.ToolAnnotations{
		Title:           a.Title,
		ReadOnlyHint:    a.ReadOnlyHint,
		DestructiveHint: a.DestructiveHint,
		IDempotentHint:  a.IDempotentHint,
		OpenWorldHint:   a.OpenWorldHint,
	}
}

// readOnlyHint extracts the readOnlyHint from broker annotations for the
// foreign-server classifier. Absent annotations yield a nil hint.
func readOnlyHint(a *broker.ToolAnnotations) *bool {
	if a == nil {
		return nil
	}
	return a.ReadOnlyHint
}

// classifiedError preserves the existing human-readable error message while
// keeping the actionable classification reachable through the embedded
// public field for the audit log.
type classifiedError struct {
	message string
	audit.Classification
	cause error
}

func (e *classifiedError) Error() string { return e.message }

func (e *classifiedError) Unwrap() error { return e.cause }

func classifyError(err error) audit.Classification {
	var classified *classifiedError
	var rpcErr *broker.RPCError
	var timeoutErr *broker.TimeoutError
	var closedErr *broker.ClosedError
	switch {
	case errors.As(err, &classified):
		return classified.Classification
	case errors.As(err, &rpcErr):
		return audit.Classification{Category: "rpc", Retryable: false}
	case errors.As(err, &timeoutErr):
		return audit.Classification{Category: "timeout", Retryable: true}
	case errors.As(err, &closedErr):
		return audit.Classification{Category: "closed", Retryable: true}
	default:
		return audit.Classification{Category: "internal", Retryable: false}
	}
}

func wrapClassifiedError(err error) error {
	if err == nil {
		return nil
	}
	if classified, ok := err.(*classifiedError); ok {
		return classified
	}
	return &classifiedError{
		message:        err.Error(),
		Classification: classifyError(err),
		cause:          err,
	}
}

// routeToolCall strips the namespace prefix from the catalog tool name,
// finds the owning child server, and forwards the call. Errors are classified
// while retaining their existing human-readable messages.
func (s *Server) routeToolCall(ctx context.Context, entry catalog.Entry, input json.RawMessage) (any, error) {
	originalName := entry.OriginalName

	ms, ok := s.servers[entry.Server]
	if !ok {
		return nil, wrapClassifiedError(fmt.Errorf("server %q not found", entry.Server))
	}

	forwardedInput, err := s.injectIdentity(entry.Server, input)
	if err != nil {
		return nil, wrapClassifiedError(err)
	}
	result, err := ms.CallTool(ctx, originalName, forwardedInput)
	if err != nil {
		return nil, wrapClassifiedError(err)
	}

	if result.IsError {
		return nil, &classifiedError{
			message:        fmt.Sprintf("tool error: %s", joinContent(result.Content)),
			Classification: audit.Classification{Category: "tool", Retryable: false},
		}
	}

	return joinContent(result.Content), nil
}

// injectIdentity applies the explicit backend mapping to a tool-call argument
// object with caller-wins precedence: a parameter already supplied by the
// caller is left untouched, and the profile name is injected only when the
// parameter is absent. Calls to unmapped backends and calls with the feature
// disabled return the original bytes unchanged so existing forwarding
// behavior is preserved.
func (s *Server) injectIdentity(alias string, input json.RawMessage) (json.RawMessage, error) {
	if s.cfg != nil && !s.cfg.Gateway.IdentityInjection {
		return input, nil
	}
	parameter, ok := policy.IdentityParameter(alias)
	if !ok {
		return input, nil
	}

	args := make(map[string]json.RawMessage)
	if len(input) > 0 {
		if err := json.Unmarshal(input, &args); err != nil {
			return nil, fmt.Errorf("gateway: decode arguments for %s: %w", alias, err)
		}
	}
	if _, exists := args[parameter]; exists {
		return input, nil
	}
	profileName, err := json.Marshal(s.profile.Name)
	if err != nil {
		return nil, fmt.Errorf("gateway: encode injected value: %w", err)
	}
	args[parameter] = profileName

	forwarded, err := json.Marshal(args)
	if err != nil {
		return nil, fmt.Errorf("gateway: encode arguments for %s: %w", alias, err)
	}
	return forwarded, nil
}

// joinContent joins the text of all content blocks with newline separators.
func joinContent(content []broker.ContentBlock) string {
	var sb strings.Builder
	for _, block := range content {
		if sb.Len() > 0 {
			sb.WriteString("\n")
		}
		sb.WriteString(block.Text)
	}
	return sb.String()
}

// bootstrapToolDescription is the imperative orientation instruction a
// fresh harness sees in tools/list. The tool itself must be called first
// in every session: it returns what this profile exposes and which tools
// exist right now (names only — vault values are never included).
const bootstrapToolDescription = "Call this first in every session. " +
	"Returns the active profile's exposure summary (which cores and tool sets are available) " +
	"and the live tool catalog (names only — vault values are never included)."

// bootstrapResponse is the structured payload of the gateway-owned
// bootstrap tool. Field names are snake_case per the repo's JSON contract.
type bootstrapResponse struct {
	Profile            string            `json:"profile"`
	ProfileDescription string            `json:"profile_description,omitempty"`
	GeneratedAt        string            `json:"generated_at"`
	Servers            []bootstrapServer `json:"servers"`
	Catalog            []string          `json:"catalog"`
	Vault              bootstrapVault    `json:"vault"`
}

// bootstrapServer summarizes one state core's exposure under the active
// profile. ExposedTools lists the namespaced tool names the harness can
// actually call on that server.
type bootstrapServer struct {
	Server       string   `json:"server"`
	Enabled      bool     `json:"enabled"`
	Mode         string   `json:"mode,omitempty"`
	ExposedTools []string `json:"exposed_tools"`
	ExposedCount int      `json:"exposed_count"`
}

// bootstrapVault reports vault presence without ever touching the child:
// a name-only entry listing would require an unlocked call, and bootstrap
// degrades to a status note instead of prompting (see issue #185).
type bootstrapVault struct {
	Status  string `json:"status"`
	Listing string `json:"listing"`
}

// handleBootstrap implements the bootstrap tool. It is deliberately a
// pure read of in-memory state assembled by buildCatalog: the call is
// cheap, never blocks on a child, never triggers a vault unlock prompt,
// and needs no caching because the catalog is immutable per connection.
func (s *Server) handleBootstrap(_ context.Context, _ json.RawMessage) (any, error) {
	resp := bootstrapResponse{
		Profile:            s.profile.Name,
		ProfileDescription: s.profile.Description,
		GeneratedAt:        time.Now().UTC().Format(time.RFC3339),
		Catalog:            s.cat.Names(),
	}

	perServer := make(map[string][]string)
	for _, entry := range s.cat.All() {
		if entry.Verdict == policy.Exposed {
			perServer[entry.Server] = append(perServer[entry.Server], entry.Name)
		}
	}

	for _, alias := range []string{profile.ServerVault, profile.ServerMemory, profile.ServerSkills} {
		cfg := s.profile.Server(alias)
		tools := perServer[alias]
		resp.Servers = append(resp.Servers, bootstrapServer{
			Server:       alias,
			Enabled:      cfg.Enabled,
			Mode:         cfg.Mode,
			ExposedTools: tools,
			ExposedCount: len(tools),
		})
	}

	resp.Vault = bootstrapVault{
		Status:  s.vaultStatus(),
		Listing: "unavailable-without-unlock",
	}
	return resp, nil
}

// vaultStatus derives vault's reportable presence without calling the
// child: disabled or mode "off" reports "disabled", enabled and reachable
// at catalog build reports "present", enabled but absent from the live
// server set (e.g. binary not found at spawn) reports "absent".
func (s *Server) vaultStatus() string {
	cfg := s.profile.Server(profile.ServerVault)
	if !cfg.Enabled || cfg.Mode == profile.VaultModeOff {
		return "disabled"
	}
	if _, ok := s.servers[profile.ServerVault]; !ok {
		return "absent"
	}
	return "present"
}

// getAIUsageToolDescription is the exposure surface for the AI usage
// report: per-provider configured/auth status plus usage snapshots,
// matching `symbrain usage --output json` (schema version 1, issue #289).
const getAIUsageToolDescription = "Fetch AI subscription/token usage across providers " +
	"(Claude, Codex, Copilot, Cursor, Kimi, Moonshot, Nous Portal, OpenCode, OpenRouter, Antigravity). " +
	"Returns the schema-versioned usage report. Read-only."

// handleAIUsage implements the get_ai_usage tool: it builds the usage
// report with a bounded timeout and returns it as JSON. Unconfigured
// providers are reported as not set up, never as errors.
func (s *Server) handleAIUsage(ctx context.Context, _ json.RawMessage) (any, error) {
	ctx, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()
	return usage.BuildReport(ctx, usage.AllProviders(nil)), nil
}

// patternsToolDescription is the read-only exposure surface for promoted
// patterns: recurring tool sequences become portable agent context. Brain
// exposes them; it never executes them.
const patternsToolDescription = "List promoted patterns for this profile: " +
	"tool-call sequences that recurred across multiple sessions, with their trigger " +
	"conditions and provenance. Read-only — patterns are never executed by symbrain."

// episodeRecorder accumulates the ordered tool-call sequence of one
// gateway session (names only — never arguments or values) for
// promotion into patterns.
type episodeRecorder struct {
	profile   string
	startedAt string
	mu        sync.Mutex
	steps     []patterns.Step
}

func newEpisodeRecorder(profileName string) *episodeRecorder {
	return &episodeRecorder{
		profile:   profileName,
		startedAt: time.Now().UTC().Format(time.RFC3339),
	}
}

// Add records one forwarded tool invocation.
func (r *episodeRecorder) Add(server, tool string) {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.steps = append(r.steps, patterns.Step{Server: server, Tool: tool})
}

// Episode returns the recorded sequence as a completed episode.
func (r *episodeRecorder) Episode() patterns.Episode {
	r.mu.Lock()
	defer r.mu.Unlock()
	return patterns.Episode{
		Profile:   r.profile,
		Steps:     append([]patterns.Step(nil), r.steps...),
		StartedAt: r.startedAt,
		EndedAt:   time.Now().UTC().Format(time.RFC3339),
	}
}

// patternsEnabled reports whether episode recording is active. It is off
// when no config is attached (tests) or when [patterns] enabled=false.
func (s *Server) patternsEnabled() bool {
	return s.cfg != nil && s.cfg.Patterns.Enabled
}

// patternsThreshold returns the configured promotion threshold, falling
// back to the package default when unset or invalid.
func (s *Server) patternsThreshold() int {
	if s.cfg != nil && s.cfg.Patterns.PromotionThreshold > 0 {
		return s.cfg.Patterns.PromotionThreshold
	}
	return config.Defaults().Patterns.PromotionThreshold
}

// flushEpisode persists one completed session's sequence into the
// profile's episode store. Empty sessions are skipped; failures are
// logged, never fatal — behavioral history is best-effort.
func (s *Server) flushEpisode(rec *episodeRecorder) {
	ep := rec.Episode()
	if len(ep.Steps) == 0 {
		return
	}
	if err := appendEpisode(ep); err != nil {
		s.logger.Warn("patterns: failed to store episode", "error", err)
	}
}

// appendEpisode writes an episode to <data dir>/patterns/<profile>.jsonl.
// It is a package variable (test seam) so tests can redirect the store
// without touching the real XDG data dir.
var appendEpisode = func(ep patterns.Episode) error {
	dir, err := xdg.PatternsDir()
	if err != nil {
		return err
	}
	store := patterns.NewPrivateStore(filepath.Join(dir, ep.Profile+".jsonl"))
	return store.Append(ep)
}

// handlePatterns implements the patterns tool: it loads the active
// profile's episodes, promotes recurring sequences against the
// configured threshold, and returns the patterns as read-only context.
func (s *Server) handlePatterns(_ context.Context, _ json.RawMessage) (any, error) {
	threshold := s.patternsThreshold()

	dir, err := xdg.PatternsDir()
	if err != nil {
		return nil, err
	}
	store := patterns.NewPrivateStore(filepath.Join(dir, s.profile.Name+".jsonl"))
	episodes, err := store.Load()
	if err != nil {
		return nil, err
	}

	return struct {
		Profile   string             `json:"profile"`
		Threshold int                `json:"threshold"`
		Patterns  []patterns.Pattern `json:"patterns"`
	}{
		Profile:   s.profile.Name,
		Threshold: threshold,
		Patterns:  patterns.Promote(episodes, threshold),
	}, nil
}
