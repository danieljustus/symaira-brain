package gateway

import (
	"context"
	"errors"
	"fmt"

	"github.com/danieljustus/symaira-brain/internal/audit"
	"github.com/danieljustus/symaira-brain/internal/broker"
	"github.com/danieljustus/symaira-brain/internal/catalog"
	"github.com/danieljustus/symaira-brain/internal/policy"
	"github.com/danieljustus/symaira-brain/internal/profile"
	"github.com/danieljustus/symaira-corekit/mcpserver"
)

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
