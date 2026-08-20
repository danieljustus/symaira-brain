package llm

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"strings"
	"time"

	"github.com/danieljustus/symaira-corekit/ollamakit"
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

type Client struct {
	OllamaURL   string
	OllamaModel string
	HTTPClient  *http.Client
	ollama      *ollamakit.Client
}

func NewClient(ollamaURL, ollamaModel string, timeout time.Duration) *Client {
	if ollamaURL == "" {
		ollamaURL = "http://localhost:11434/api/generate"
	}
	if ollamaModel == "" {
		ollamaModel = "llama3"
	}
	if timeout <= 0 {
		timeout = 45 * time.Second
	}
	c := &Client{
		OllamaURL:   ollamaURL,
		OllamaModel: ollamaModel,
		HTTPClient: &http.Client{
			Timeout: timeout,
		},
	}
	c.ollama = ollamakit.New(ollamakit.Config{
		BaseURL: ollamaBaseURL(ollamaURL),
		Model:   ollamaModel,
		Timeout: timeout,
	})
	return c
}

// ollamaBaseURL strips a configured Ollama endpoint path (e.g.
// "http://localhost:11434/api/generate") down to the scheme+host root
// ollamakit.Config.BaseURL expects. Malformed input is passed through
// unchanged so ollamakit's own defaulting takes over.
func ollamaBaseURL(raw string) string {
	u, err := url.Parse(raw)
	if err != nil || u.Scheme == "" || u.Host == "" {
		return raw
	}
	return u.Scheme + "://" + u.Host
}

func (c *Client) QueryOllama(ctx context.Context, systemPrompt, userPrompt string) (string, error) {
	return c.QueryOllamaWithSchema(ctx, systemPrompt, userPrompt, ConsolidationResponseSchema())
}

// QueryOllamaWithSchema queries a local Ollama endpoint with an explicit
// JSON-Schema (draft-07) constraint for the response. Unlike QueryOllama,
// the caller controls the schema, which is required for response types
// other than consolidation results.
func (c *Client) QueryOllamaWithSchema(ctx context.Context, systemPrompt, userPrompt string, schema map[string]any) (string, error) {
	body := map[string]any{
		"model":  c.OllamaModel,
		"prompt": userPrompt,
		"system": systemPrompt,
		"stream": true,
		"format": schema,
	}

	var reqBuf bytes.Buffer
	if err := json.NewEncoder(&reqBuf).Encode(body); err != nil {
		return "", fmt.Errorf("failed to encode Ollama request: %w", err)
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.OllamaURL, &reqBuf)
	if err != nil {
		return "", fmt.Errorf("failed to create Ollama request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")

	resp, err := c.HTTPClient.Do(req)
	if err != nil {
		return "", fmt.Errorf("failed to query Ollama: %w", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusOK {
		respBody, _ := io.ReadAll(io.LimitReader(resp.Body, 4<<10))
		return "", fmt.Errorf("ollama returned HTTP %d: %s", resp.StatusCode, strings.TrimSpace(string(respBody)))
	}

	var out strings.Builder
	scanner := bufio.NewScanner(resp.Body)
	scanner.Buffer(make([]byte, 0, 64*1024), 1024*1024)
	for scanner.Scan() {
		line := scanner.Bytes()
		if len(line) == 0 {
			continue
		}
		var chunk struct {
			Response string `json:"response"`
			Done     bool   `json:"done"`
		}
		if err := json.Unmarshal(line, &chunk); err != nil {
			return "", fmt.Errorf("failed to decode Ollama response chunk: %w", err)
		}
		out.WriteString(chunk.Response)
		if chunk.Done {
			break
		}
	}
	if err := scanner.Err(); err != nil {
		return "", fmt.Errorf("ollama stream read error: %w", err)
	}

	return out.String(), nil
}

func (c *Client) QueryOpenAI(ctx context.Context, systemPrompt, userPrompt, apiKey, model, url string) (string, error) {
	if model == "" {
		model = "gpt-4o-mini"
	}
	if url == "" {
		url = "https://api.openai.com/v1/chat/completions"
	}

	schema := ConsolidationResponseSchema()
	reqBody, err := json.Marshal(map[string]interface{}{
		"model": model,
		"messages": []map[string]string{
			{"role": "system", "content": systemPrompt},
			{"role": "user", "content": userPrompt},
		},
		"response_format": map[string]interface{}{
			"type": "json_schema",
			"json_schema": map[string]interface{}{
				"name":   "consolidation_result",
				"strict": true,
				"schema": schema,
			},
		},
	})
	if err != nil {
		return "", err
	}

	req, err := http.NewRequestWithContext(ctx, "POST", url, bytes.NewBuffer(reqBody))
	if err != nil {
		return "", err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+apiKey)

	resp, err := c.HTTPClient.Do(req)
	if err != nil {
		return "", fmt.Errorf("failed to query OpenAI: %w", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusOK {
		return "", fmt.Errorf("openai returned HTTP status %d", resp.StatusCode)
	}

	var res struct {
		Choices []struct {
			Message struct {
				Content string `json:"content"`
			} `json:"message"`
		} `json:"choices"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&res); err != nil {
		return "", err
	}

	if len(res.Choices) == 0 {
		return "", fmt.Errorf("openai returned empty choices")
	}

	return res.Choices[0].Message.Content, nil
}

func (c *Client) Query(ctx context.Context, systemPrompt, userPrompt, provider, apiKey string) (string, error) {
	if provider == "openai" {
		if apiKey == "" {
			apiKey = os.Getenv("OPENAI_API_KEY")
		}
		if apiKey == "" {
			return "", fmt.Errorf("OPENAI_API_KEY environment variable is not set")
		}
		return c.QueryOpenAI(ctx, systemPrompt, userPrompt, apiKey, "", "")
	}
	return c.QueryOllama(ctx, systemPrompt, userPrompt)
}
