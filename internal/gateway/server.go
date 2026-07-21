package gateway

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log/slog"

	"github.com/danieljustus/symaira-brain/internal/broker"
	"github.com/danieljustus/symaira-brain/internal/catalog"
	"github.com/danieljustus/symaira-brain/internal/policy"
	"github.com/danieljustus/symaira-brain/internal/profile"
	"github.com/danieljustus/symaira-corekit/mcpserver"
)

// Server is the MCP gateway: it presents a merged, policy-filtered tool
// catalog to the harness and routes tools/call requests to the owning
// child server by stripping the namespace prefix.
type Server struct {
	profile *profile.Profile
	servers map[string]*broker.ManagedServer
	cat     *catalog.Catalog
	logger  *slog.Logger
}

// New creates a gateway Server from a profile and pre-built managed
// servers. The catalog is built lazily on the first ServeIO call (since
// tools/list requires live child connections).
func New(p *profile.Profile, servers map[string]*broker.ManagedServer, logger *slog.Logger) *Server {
	if logger == nil {
		logger = slog.Default()
	}
	return &Server{
		profile: p,
		servers: servers,
		logger:  logger,
	}
}

// ServeIO serves the MCP protocol over the given reader/writer pair
// (stdin/stdout). It blocks until the client disconnects or ctx is
// cancelled.
func (s *Server) ServeIO(ctx context.Context, r io.Reader, w io.Writer) error {
	if err := s.buildCatalog(ctx); err != nil {
		return fmt.Errorf("gateway: build catalog: %w", err)
	}

	srv := mcpserver.New("symbrain", "dev")
	srv.SetInstructions(fmt.Sprintf("symbrain profile %q", s.profile.Name))

	for _, entry := range s.cat.Exposed() {
		entry := entry
		srv.RegisterTool(&mcpserver.Tool{
			Name:        entry.Name,
			Description: entry.Description,
			InputSchema: entry.InputSchema,
			Handler: func(ctx context.Context, input json.RawMessage) (any, error) {
				return s.routeToolCall(ctx, entry, input)
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

// routeToolCall strips the namespace prefix from the catalog tool name,
// finds the owning child server, and forwards the call. Errors are
// mapped faithfully: RPC errors, timeouts, and tool-level errors are all
// preserved.
func (s *Server) routeToolCall(ctx context.Context, entry catalog.Entry, input json.RawMessage) (any, error) {
	originalName := entry.OriginalName

	var args any
	if len(input) > 0 {
		if err := json.Unmarshal(input, &args); err != nil {
			return nil, fmt.Errorf("invalid arguments: %w", err)
		}
	}

	ms, ok := s.servers[entry.Server]
	if !ok {
		return nil, fmt.Errorf("server %q not found", entry.Server)
	}

	result, err := ms.CallTool(ctx, originalName, input)
	if err != nil {
		return nil, err
	}

	if result.IsError {
		text := ""
		if len(result.Content) > 0 {
			text = result.Content[0].Text
		}
		return nil, fmt.Errorf("tool error: %s", text)
	}

	if len(result.Content) == 0 {
		return map[string]any{}, nil
	}
	return result.Content[0].Text, nil
}
