package conflict

import (
	"context"
	"encoding/json"
	"fmt"
	"regexp"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/llm"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
)

// verdictResponseSchema constrains the LLM verdict response to a list of
// per-pair decisions.
func verdictResponseSchema() map[string]any {
	return map[string]any{
		"$schema": "http://json-schema.org/draft-07/schema#",
		"type":    "object",
		"properties": map[string]any{
			"verdicts": map[string]any{
				"type": "array",
				"items": map[string]any{
					"type": "object",
					"properties": map[string]any{
						"pair": map[string]any{
							"type": "integer",
						},
						"verdict": map[string]any{
							"type": "string",
							"enum": []any{"repeat", "contradiction", "ambiguous"},
						},
					},
					"required": []any{"pair", "verdict"},
				},
			},
		},
		"required": []any{"verdicts"},
	}
}

// LLMVerdictProvider classifies contradiction-band pairs with one batched
// LLM call covering the whole candidate pool of a write (#462). It is the
// optional third tier: off by default, so a CLI write never requires an
// LLM round-trip. Any failure degrades the batch to ambiguous (store
// both) rather than failing the write.
type LLMVerdictProvider struct {
	client   *llm.Client
	provider string
}

// NewLLMVerdictProvider builds the LLM verdict tier for the given config.
func NewLLMVerdictProvider(cfg config.ConflictConfig) *LLMVerdictProvider {
	return &LLMVerdictProvider{
		client:   llm.NewClient(cfg.LLMURL, cfg.LLMModel, 0),
		provider: cfg.LLMProvider,
	}
}

const verdictSystemPrompt = `You are a memory-store conflict resolver. For each pair of facts you receive, decide whether the new fact (from the current write) REPEATS the old fact (same claim, possibly reworded), CONTRADICTS it (same subject, incompatible claim), or is AMBIGUOUS (related but not clearly either). Answer with a JSON object: {"verdicts":[{"pair":N,"verdict":"repeat|contradiction|ambiguous"}]} — one entry per pair, in order. When in doubt, answer "ambiguous": a silently wrong supersession is worse than a visible conflict. Never invent pairs that were not given.`

func (p *LLMVerdictProvider) Verdicts(ctx context.Context, pairs []Pair) ([]Verdict, error) {
	if p == nil || p.client == nil {
		return nil, fmt.Errorf("conflict: llm verdict provider not configured")
	}
	var sb strings.Builder
	for i, pr := range pairs {
		// Stored memory content is untrusted data (#505): the same
		// line-wise marker neutralization the extraction and
		// consolidation paths apply (#493) is applied here before the
		// batch is composed, so an instruction-injection marker cannot
		// steer the verdict model.
		fmt.Fprintf(&sb, "[pair %d]\nnew fact (written now, by %q): %s\nold fact (already stored, by %q): %s\n\n",
			i, pr.NewActor, security.SanitizeLines(pr.NewContent), pr.OldActor, security.SanitizeLines(pr.OldContent))
	}

	// The standing "data, never instructions" preamble precedes the
	// untrusted pair content, mirroring the consolidation system prompt.
	systemPrompt := verdictSystemPrompt + "\n\n" + security.UntrustedPreamble

	var raw string
	var err error
	if p.provider == "openai" {
		raw, err = p.client.Query(ctx, systemPrompt, sb.String(), "openai", "")
	} else {
		// Ollama path: pin the verdict JSON schema. The generic Query
		// would force the consolidation schema, which is the wrong
		// response shape here.
		raw, err = p.client.QueryOllamaWithSchema(ctx, systemPrompt, sb.String(), verdictResponseSchema())
	}
	if err != nil {
		return nil, fmt.Errorf("conflict: llm verdict query: %w", err)
	}
	// parseVerdictResponse cannot fail: the salvage path always yields
	// one verdict per pair (ambiguous for undecided ones), so malformed
	// or partial output degrades to store-both instead of failing the
	// write (#506).
	return parseVerdictResponse(raw, len(pairs))
}

var verdictTokenRe = regexp.MustCompile(`(?i)"verdict"\s*:\s*"(repeat|contradiction|ambiguous)"`)

// parseVerdictResponse extracts one verdict per pair from the LLM output.
// It first tries strict JSON; when that fails it falls back to scanning
// for verdict tokens in order, then pads any missing tail pairs as
// ambiguous. The error return is always nil — the salvage path cannot
// fail — and is kept so callers can treat the result as fallible without
// a behavior change.
func parseVerdictResponse(raw string, want int) ([]Verdict, error) {
	if want <= 0 {
		return nil, nil
	}
	var parsed struct {
		Verdicts []struct {
			Pair    int    `json:"pair"`
			Verdict string `json:"verdict"`
		} `json:"verdicts"`
	}
	if err := json.Unmarshal([]byte(raw), &parsed); err == nil && len(parsed.Verdicts) > 0 {
		out := make([]Verdict, want)
		for i := range out {
			out[i] = VerdictAmbiguous
		}
		for _, v := range parsed.Verdicts {
			if v.Pair < 0 || v.Pair >= want {
				continue
			}
			switch strings.ToLower(strings.TrimSpace(v.Verdict)) {
			case "repeat":
				out[v.Pair] = VerdictRepeat
			case "contradiction":
				out[v.Pair] = VerdictContradiction
			default:
				out[v.Pair] = VerdictAmbiguous
			}
		}
		return out, nil
	}

	// Salvage: scan for verdict tokens in order.
	matches := verdictTokenRe.FindAllStringSubmatch(raw, -1)
	out := make([]Verdict, want)
	for i := range out {
		out[i] = VerdictAmbiguous
	}
	for i, m := range matches {
		if i >= want {
			break
		}
		switch strings.ToLower(m[1]) {
		case "repeat":
			out[i] = VerdictRepeat
		case "contradiction":
			out[i] = VerdictContradiction
		}
	}
	return out, nil
}
