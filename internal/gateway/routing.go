package gateway

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/audit"
	"github.com/danieljustus/symaira-brain/internal/broker"
	"github.com/danieljustus/symaira-brain/internal/catalog"
	"github.com/danieljustus/symaira-brain/internal/policy"
)

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
