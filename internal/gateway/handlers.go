package gateway

import (
	"context"
	"encoding/json"
	"path/filepath"
	"sync"
	"time"

	"github.com/danieljustus/symaira-brain/internal/config"
	"github.com/danieljustus/symaira-brain/internal/patterns"
	"github.com/danieljustus/symaira-brain/internal/policy"
	"github.com/danieljustus/symaira-brain/internal/profile"
	"github.com/danieljustus/symaira-brain/internal/usage"
	"github.com/danieljustus/symaira-brain/internal/xdg"
)

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

	for _, alias := range []string{profile.ServerVault, profile.ServerMemory, profile.ServerSkills, profile.ServerUsage, profile.ServerOperate, profile.ServerScope} {
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
