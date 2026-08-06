package gateway

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"strings"
	"sync"
	"time"

	"github.com/danieljustus/symaira-brain/internal/audit"
	"github.com/danieljustus/symaira-brain/internal/broker"
	"github.com/danieljustus/symaira-brain/internal/catalog"
	"github.com/danieljustus/symaira-brain/internal/config"
	"github.com/danieljustus/symaira-brain/internal/policy"
	"github.com/danieljustus/symaira-brain/internal/profile"
	"github.com/danieljustus/symaira-corekit/mcpserver"
)

// Server is the MCP gateway: it presents a merged, policy-filtered tool
// catalog to the harness and routes tools/call requests to the owning
// child server by stripping the namespace prefix.
type Server struct {
	profile           *profile.Profile
	servers           map[string]*broker.ManagedServer
	cat               *catalog.Catalog
	logger            *slog.Logger
	cfg               *config.Config
	version           string
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

// auditSink is the subset of the audit.Logger API that ServeIO uses. It
// is an interface so tests can substitute a fake logger (e.g. to force
// the degraded-warning path) without changing production behavior.
type auditSink interface {
	Log(server, tool string, args json.RawMessage, duration time.Duration, status string, classifications ...audit.Classification)
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
		}
	}

	srv := mcpserver.New("symbrain", s.version)
	srv.SetInstructions(fmt.Sprintf("symbrain profile %q", s.profile.Name))

	for _, entry := range s.cat.Exposed() {
		entry := entry
		srv.RegisterTool(&mcpserver.Tool{
			Name:        entry.Name,
			Description: entry.Description,
			InputSchema: entry.InputSchema,
			Handler: func(ctx context.Context, input json.RawMessage) (any, error) {
				start := time.Now()
				result, err := s.routeToolCall(ctx, entry, input)
				if auditLog != nil {
					status := "ok"
					var classification audit.Classification
					if err != nil {
						status = "error"
						if classified, ok := err.(*classifiedError); ok {
							classification = classified.Classification
						}
					}
					auditLog.Log(entry.Server, entry.OriginalName, input, time.Since(start), status, classification)
					if auditLog.Degraded() {
						s.auditDegradedWarn.Do(func() {
							s.logger.Warn("audit log degraded; some entries may not be persisted")
						})
					}
				}
				return result, err
			},
		})
	}

	return srv.ServeIO(ctx, r, w)
}

// buildCatalog queries each managed server for its tools, evaluates the
// policy, and builds the merged catalog. It must be called before
// registering tools with mcpserver.
func (s *Server) buildCatalog(ctx context.Context) error {
	var servers []catalog.ServerTools

	for alias, ms := range s.servers {
		serverCfg := s.profile.Server(alias)
		if !serverCfg.Enabled {
			continue
		}

		tools, err := ms.ListTools(ctx)
		if err != nil {
			s.logger.Warn("failed to list tools from child",
				"server", alias, "error", err)
			continue
		}

		brokerTools := make([]catalog.Tool, len(tools))
		for i, t := range tools {
			brokerTools[i] = catalog.Tool{
				Name:        t.Name,
				Description: t.Description,
				InputSchema: t.InputSchema,
			}
		}

		liveNames := make([]string, len(tools))
		for i, t := range tools {
			liveNames[i] = t.Name
		}

		report, err := policy.Evaluate(alias, serverCfg, liveNames)
		if err != nil {
			return fmt.Errorf("gateway: evaluate policy for %s: %w", alias, err)
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

// classifiedError preserves the existing human-readable error message while
// exposing the actionable category pair in the MCP tool-result text.
type classifiedError struct {
	message string
	audit.Classification
	cause error
}

func (e *classifiedError) Error() string {
	payload := struct {
		Message string `json:"message"`
		audit.Classification
	}{Message: e.message, Classification: e.Classification}
	data, err := json.Marshal(payload)
	if err != nil {
		return e.message
	}
	return string(data)
}

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
// object. Calls to unmapped backends and calls with the feature disabled return
// the original bytes unchanged so existing forwarding behavior is preserved.
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
	profileName, err := json.Marshal(s.profile.Name)
	if err != nil {
		return nil, fmt.Errorf("gateway: encode profile identity: %w", err)
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
