package gateway

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/danieljustus/symaira-brain/internal/audit"
	"github.com/danieljustus/symaira-brain/internal/broker"
	"github.com/danieljustus/symaira-brain/internal/catalog"
	"github.com/danieljustus/symaira-brain/internal/config"
	memorymcp "github.com/danieljustus/symaira-brain/internal/memory/mcp"
	"github.com/danieljustus/symaira-brain/internal/policy"
	"github.com/danieljustus/symaira-brain/internal/profile"
	"github.com/danieljustus/symaira-brain/internal/skills/mcptools"
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
			Handler:     s.wrapInProcess(auditLog, recorder, "usage")("get_ai_usage", s.handleAIUsage),
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
