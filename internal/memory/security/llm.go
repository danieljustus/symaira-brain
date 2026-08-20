package security

import (
	"bytes"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

// LLMEnhancer connects to local or cloud LLMs for background memory cleanup and merging.
//
// Deprecated: Use internal/llm.Client instead. This type is retained for backward
// compatibility and will be removed in a future version.
type LLMEnhancer struct {
	OllamaURL string
	Model     string
}

// NewLLMEnhancer creates a configuration object.
func NewLLMEnhancer() *LLMEnhancer {
	return &LLMEnhancer{
		OllamaURL: "http://localhost:11434/api/generate",
		Model:     "llama3", // default local Ollama model
	}
}

// ConsolidateMemories takes a slice of memories, identifies duplicates or conflicts,
// and prompts the LLM to clean them into a consolidated list of distinct facts.
func (le *LLMEnhancer) ConsolidateMemories(memories []*db.Memory) ([]string, error) {
	if len(memories) <= 1 {
		var list []string
		for _, m := range memories {
			list = append(list, m.Content)
		}
		return list, nil
	}

	// Prepare list of statements for the LLM. Every statement is treated as
	// untrusted data (#493): injection markers are neutralized before the
	// prompt is composed.
	var builder strings.Builder
	builder.WriteString("Statements:\n")
	for i, m := range memories {
		fmt.Fprintf(&builder, "%d. %s (Scope: %s)\n", i+1, SanitizeLines(m.Content), m.Scope)
	}

	prompt := fmt.Sprintf(`You are the semantic memory cleaning engine for Symaira Memory. 
Analyze the following list of stored facts about the user and clean them.
Your tasks:
1. Merge duplicate statements.
2. Resolve conflicting information (e.g. if one says User hates Go, and another says User prefers Go, resolve it by prioritizing modern/positive assertions).
3. output a concise list of cleaned, standalone, atomic facts.

%s

Format your response strictly as a JSON array of strings, like this:
[
  "atomic fact 1",
  "atomic fact 2"
]
Do not include any introductory or concluding text. Respond ONLY with the JSON array.`, builder.String())

	// Try cloud APIs first if configured, otherwise fall back to Ollama.
	var facts []string
	var err error

	if apiKey := os.Getenv("OPENAI_API_KEY"); apiKey != "" {
		facts, err = le.queryOpenAI(prompt, apiKey)
	} else {
		facts, err = le.queryOllama(prompt)
	}

	// Graceful fallback: if no LLM is available, return original content unchanged.
	if err != nil {
		var list []string
		for _, m := range memories {
			list = append(list, m.Content)
		}
		return list, nil
	}

	return facts, nil
}

func (le *LLMEnhancer) queryOllama(prompt string) ([]string, error) {
	client := &http.Client{Timeout: 15 * time.Second}

	reqBody, err := json.Marshal(map[string]interface{}{
		"model":  le.Model,
		"prompt": prompt,
		"stream": false,
		"format": "json", // request JSON format
	})
	if err != nil {
		return nil, err
	}

	resp, err := client.Post(le.OllamaURL, "application/json", bytes.NewBuffer(reqBody))
	if err != nil {
		return nil, fmt.Errorf("ollama daemon connection failed: %w", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("ollama returned HTTP error: %d", resp.StatusCode)
	}

	var res struct {
		Response string `json:"response"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&res); err != nil {
		return nil, err
	}

	var facts []string
	if err := json.Unmarshal([]byte(res.Response), &facts); err != nil {
		// Fallback if parsing fails: split by newlines
		return parseFallbackList(res.Response), nil
	}

	return facts, nil
}

func (le *LLMEnhancer) queryOpenAI(prompt string, apiKey string) ([]string, error) {
	client := &http.Client{Timeout: 10 * time.Second}

	reqBody, err := json.Marshal(map[string]interface{}{
		"model": "gpt-4o-mini",
		"messages": []map[string]string{
			{"role": "user", "content": prompt},
		},
		"response_format": map[string]string{"type": "json_object"},
	})
	if err != nil {
		return nil, err
	}

	req, _ := http.NewRequest("POST", "https://api.openai.com/v1/chat/completions", bytes.NewBuffer(reqBody))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+apiKey)

	resp, err := client.Do(req)
	if err != nil {
		return nil, err
	}
	defer func() { _ = resp.Body.Close() }()

	var res struct {
		Choices []struct {
			Message struct {
				Content string `json:"content"`
			} `json:"message"`
		} `json:"choices"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&res); err != nil {
		return nil, err
	}

	if len(res.Choices) == 0 {
		return nil, fmt.Errorf("openai returned empty choice list")
	}

	var facts []string
	content := res.Choices[0].Message.Content
	// If OpenAI nested it in an object, try parsing list
	var wrapper struct {
		Facts []string `json:"facts"`
	}
	if err := json.Unmarshal([]byte(content), &wrapper); err == nil && len(wrapper.Facts) > 0 {
		return wrapper.Facts, nil
	}

	if err := json.Unmarshal([]byte(content), &facts); err != nil {
		return parseFallbackList(content), nil
	}

	return facts, nil
}

func parseFallbackList(s string) []string {
	var list []string
	lines := strings.Split(s, "\n")
	for _, l := range lines {
		l = strings.TrimSpace(l)
		l = strings.Trim(l, `[]"'-*, `)
		if len(l) > 5 {
			list = append(list, l)
		}
	}
	return list
}
