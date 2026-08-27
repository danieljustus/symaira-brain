package llm

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"time"

	"github.com/danieljustus/symaira-corekit/llmkit"
)

// ConsolidationResponseSchema returns a JSON Schema (draft-07) for the
// consolidation response type. Both Ollama (format object) and OpenAI
// (response_format / json_schema) use this to constrain the LLM to emit
// valid ConsolidationResult JSON, eliminating the need for the salvage
// strategies in parseJSONResponse when the model supports schema-guided
// output.
func ConsolidationResponseSchema() map[string]any {
	return map[string]any{
		"$schema": "http://json-schema.org/draft-07/schema#",
		"type":    "object",
		"properties": map[string]any{
			"consolidated": map[string]any{
				"type": "array",
				"items": map[string]any{
					"type": "object",
					"properties": map[string]any{
						"content": map[string]any{
							"type": "string",
						},
						"replaces_ids": map[string]any{
							"type": "array",
							"items": map[string]any{
								"type": "string",
							},
						},
						"metadata": map[string]any{
							"type":                 "object",
							"additionalProperties": map[string]any{"type": "string"},
						},
					},
					"required": []any{"content", "replaces_ids", "metadata"},
				},
			},
			"discarded_ids": map[string]any{
				"type": "array",
				"items": map[string]any{
					"type": "string",
				},
			},
		},
		"required": []any{"consolidated", "discarded_ids"},
	}
}

const (
	defaultOllamaBase  = "http://localhost:11434"
	defaultOllamaModel = "llama3"
	defaultOpenAIModel = "gpt-4o-mini"
	defaultTimeout     = 45 * time.Second
)

// Client is the memory-store's thin configuration adapter over
// corekit/llmkit. It owns zero transport code: provider descriptors,
// credential resolution (env:// / symvault://) and the wire dialect
// handling all live in llmkit. What remains here is the brain-side
// choice of provider, model and response schema.
type Client struct {
	// OllamaBase is the base URL for native Ollama calls (scheme+host [+/v1]).
	OllamaBase string
	// OllamaModel is the local model name (default "llama3").
	OllamaModel string
	// LLMURL is the OpenAI-compatible base URL for the cloud path; when
	// empty the openai provider descriptor default applies.
	LLMURL string
	// LLMModel is the cloud model (default "gpt-4o-mini").
	LLMModel string
	// Provider selects the dialect: "ollama" (native) or "openai" (chat
	// completions wire). Empty auto-detects via OPENAI_API_KEY.
	Provider string
	// Timeout is the per-request timeout; <=0 means defaultTimeout.
	Timeout time.Duration

	ollama *llmkit.Client // lazy
	openai *llmkit.Client // lazy
}

// NewClient builds the llmkit-backed client. llmURL/llmModel are the
// local-endpoint configuration (as before); the cloud path uses its own
// defaults unless LLMURL/LLMModel are set explicitly.
func NewClient(llmURL, llmModel, llmProvider string, timeout time.Duration) *Client {
	c := &Client{
		OllamaBase:  llmURL,
		OllamaModel: llmModel,
		Provider:    llmProvider,
		Timeout:     timeout,
	}
	if c.OllamaBase == "" {
		c.OllamaBase = defaultOllamaBase
	}
	if c.OllamaModel == "" {
		c.OllamaModel = defaultOllamaModel
	}
	if c.Provider == "" {
		if os.Getenv("OPENAI_API_KEY") != "" {
			c.Provider = "openai"
		} else {
			c.Provider = "ollama"
		}
	}
	return c
}

// resolveTimeout applies the default when no timeout was configured.
func (c *Client) resolveTimeout() time.Duration {
	if c.Timeout <= 0 {
		return defaultTimeout
	}
	return c.Timeout
}

// ollamaClient returns the native-Ollama llmkit client, built once.
// Native calls (Generate, ChatStream) live at the scheme+host root, NOT
// under the /v1 OpenAI-compatibility prefix, so the base URL is always
// stripped to the bare host.
func (c *Client) ollamaClient() (*llmkit.Client, error) {
	if c.ollama == nil {
		desc, ok := llmkit.Lookup("ollama")
		if !ok {
			return nil, fmt.Errorf("llm: ollama descriptor missing from embedded registry")
		}
		base := ollamaRoot(c.OllamaBase)
		cl, err := llmkit.NewClient(desc, "", llmkit.WithBaseURL(base), llmkit.WithTimeout(c.resolveTimeout()))
		if err != nil {
			return nil, err
		}
		c.ollama = cl
	}
	return c.ollama, nil
}

// ollamaRoot strips any path (including the OpenAI-compatible /v1 prefix)
// from a configured endpoint, because the native /api/* surface lives at
// the scheme+host root.
func ollamaRoot(raw string) string {
	for i := 0; i < len(raw); i++ {
		if raw[i] == '/' && i > len("http://") {
			return raw[:i]
		}
	}
	return raw
}

// openaiClient returns the OpenAI-wire llmkit client, built once.
func (c *Client) openaiClient() (*llmkit.Client, error) {
	if c.openai == nil {
		desc, ok := llmkit.Lookup("openai")
		if !ok {
			return nil, fmt.Errorf("llm: openai descriptor missing from embedded registry")
		}
		opts := []llmkit.Option{llmkit.WithTimeout(c.resolveTimeout())}
		if c.LLMURL != "" {
			opts = append(opts, llmkit.WithBaseURL(c.LLMURL))
		}
		cl, err := llmkit.NewClient(desc, "", opts...)
		if err != nil {
			return nil, err
		}
		c.openai = cl
	}
	return c.openai, nil
}

// Query is the consolidation-shaped query: it pins the consolidation
// response schema on both provider paths.
func (c *Client) Query(ctx context.Context, systemPrompt, userPrompt, provider string) (string, error) {
	return c.QueryWithSchema(ctx, systemPrompt, userPrompt, provider, ConsolidationResponseSchema())
}

// QueryWithSchema queries the configured provider with an explicit
// JSON-Schema (draft-07) constraint. The provider parameter overrides the
// client-level provider for call sites that mix dialects.
func (c *Client) QueryWithSchema(ctx context.Context, systemPrompt, userPrompt, provider string, schema map[string]any) (string, error) {
	if provider == "" {
		provider = c.Provider
	}
	if provider == "openai" {
		return c.queryOpenAI(ctx, systemPrompt, userPrompt, schema)
	}
	return c.queryOllama(ctx, systemPrompt, userPrompt, schema)
}

// queryOllama streams a native /api/generate call with the schema pinned in
// Ollama's format field and the system prompt carried natively.
func (c *Client) queryOllama(ctx context.Context, systemPrompt, userPrompt string, schema map[string]any) (string, error) {
	cl, err := c.ollamaClient()
	if err != nil {
		return "", err
	}
	var out strings.Builder
	err = cl.Generate(ctx, c.OllamaModel, userPrompt, func(ch llmkit.GenerateResponse) error {
		out.WriteString(ch.Response)
		return nil
	}, llmkit.WithGenerateSystem(systemPrompt), llmkit.WithGenerateFormat(schema))
	if err != nil {
		return "", fmt.Errorf("failed to query ollama: %w", err)
	}
	return out.String(), nil
}

// queryOpenAI performs a chat completion against the OpenAI-wire endpoint
// with the schema pinned in response_format.
func (c *Client) queryOpenAI(ctx context.Context, systemPrompt, userPrompt string, schema map[string]any) (string, error) {
	cl, err := c.openaiClient()
	if err != nil {
		return "", err
	}
	model := c.LLMModel
	if model == "" {
		model = defaultOpenAIModel
	}
	rawSchema, err := json.Marshal(schema)
	if err != nil {
		return "", fmt.Errorf("failed to encode response schema: %w", err)
	}
	choice, err := cl.Chat(ctx, model, []llmkit.Message{{Role: "user", Content: userPrompt}},
		&llmkit.ChatOptions{
			System:         systemPrompt,
			ResponseFormat: rawSchema,
		})
	if err != nil {
		return "", fmt.Errorf("failed to query openai: %w", err)
	}
	return choice.Content, nil
}
