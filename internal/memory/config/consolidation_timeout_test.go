package config

import (
	"testing"
	"time"
)

// TestConsolidationParseTimeout covers ConsolidationConfig.ParseTimeout (#538),
// the configurable LLM client timeout added in the Wave-1 MCP/dream hardening.
func TestConsolidationParseTimeout(t *testing.T) {
	tests := []struct {
		name    string
		timeout string // raw config value
		want    time.Duration
	}{
		{name: "empty defaults to 10m", timeout: "", want: 10 * time.Minute},
		{name: "explicit duration", timeout: "5m", want: 5 * time.Minute},
		{name: "longer duration", timeout: "30m", want: 30 * time.Minute},
		{name: "sub-minute", timeout: "45s", want: 45 * time.Second},
		{name: "invalid falls back to 10m", timeout: "not-a-duration", want: 10 * time.Minute},
		{name: "whitespace is invalid and falls back", timeout: " 10m ", want: 10 * time.Minute},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			c := ConsolidationConfig{Timeout: tt.timeout}
			got := c.ParseTimeout()
			if got != tt.want {
				t.Fatalf("ParseTimeout(%q) = %v, want %v", tt.timeout, got, tt.want)
			}
		})
	}
}

// TestConsolidationTimeoutDefaultVerifiesDefaults ensures the default config
// carries the documented 10m timeout, so ParseTimeout's zero-value behavior
// matches the shipped default.
func TestConsolidationTimeoutDefaultVerifiesDefaults(t *testing.T) {
	cfg := Defaults()
	if cfg.Consolidation.Timeout != "10m" {
		t.Fatalf("default consolidation timeout = %q, want %q", cfg.Consolidation.Timeout, "10m")
	}
	if got := cfg.Consolidation.ParseTimeout(); got != 10*time.Minute {
		t.Fatalf("default ParseTimeout() = %v, want 10m", got)
	}
}
