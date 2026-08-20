package consolidation

import (
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

// #483: prompt families

func TestBuildSystemPromptChatUnchanged(t *testing.T) {
	// The chat family must be byte-identical to the pre-#483 wording, plus
	// the shared untrusted-data preamble (#493).
	chat := BuildSystemPrompt(PromptFamilyChat)
	if !strings.Contains(chat, "You are the Symaira Memory Consolidation Engine.") {
		t.Fatalf("chat system prompt lost its identity:\n%s", chat)
	}
	if !strings.Contains(chat, "UNTRUSTED USER DATA") {
		t.Fatalf("chat system prompt lost the untrusted-data guard:\n%s", chat)
	}
	if strings.Contains(chat, "code mode") || strings.Contains(chat, "CODING TRANSCRIPTS") {
		t.Fatalf("chat family must not contain code-family wording:\n%s", chat)
	}
}

func TestBuildSystemPromptCodeFamily(t *testing.T) {
	code := BuildSystemPrompt(PromptFamilyCode)
	for _, want := range []string{"code mode", "CODING TRANSCRIPTS", "build commands", "module boundaries", "architectural"} {
		if !strings.Contains(code, want) {
			t.Errorf("code system prompt missing %q:\n%s", want, code)
		}
	}
}

func TestBuildUserPromptChatUnchanged(t *testing.T) {
	chat := BuildUserPrompt(PromptFamilyChat, "global", "<memory_content>")
	if !strings.Contains(chat, `Analyze and consolidate raw, new memories for scope: "global"`) {
		t.Fatalf("chat user prompt header changed:\n%s", chat)
	}
	if !strings.Contains(chat, "JSON Schema") || !strings.Contains(chat, "discarded_ids") {
		t.Fatalf("chat user prompt lost the response schema:\n%s", chat)
	}
	if !strings.Contains(chat, "<memory_content>") {
		t.Fatalf("chat user prompt lost the memory block:\n%s", chat)
	}
	if strings.Contains(chat, "coding transcripts") {
		t.Fatalf("chat family must not contain code-family wording:\n%s", chat)
	}
}

func TestBuildUserPromptCodeFamily(t *testing.T) {
	code := BuildUserPrompt(PromptFamilyCode, "global", "<memory_content>")
	for _, want := range []string{"coding transcripts", "build commands", "file conventions", "module boundaries", "JSON Schema", "discarded_ids"} {
		if !strings.Contains(code, want) {
			t.Errorf("code user prompt missing %q", want)
		}
	}
}

func TestResolveFamily(t *testing.T) {
	meta := func(mode string) map[string]string {
		if mode == "" {
			return map[string]string{}
		}
		return map[string]string{PromptModeKey: mode}
	}
	memories := func(modes ...string) []*db.Memory {
		out := make([]*db.Memory, 0, len(modes))
		for _, m := range modes {
			out = append(out, &db.Memory{ID: "m", Metadata: meta(m)})
		}
		return out
	}

	// Global default applies when no group carries the code marker.
	if f := ResolveFamily(memories("", ""), "chat"); f != PromptFamilyChat {
		t.Fatalf("unmarked group must use the default, got %q", f)
	}
	// Majority code-marked → code family, overriding the default.
	if f := ResolveFamily(memories("code", "code", ""), "chat"); f != PromptFamilyCode {
		t.Fatalf("code-majority group must use code family, got %q", f)
	}
	// Minority code-marked → default wins.
	if f := ResolveFamily(memories("code", "", ""), "chat"); f != PromptFamilyChat {
		t.Fatalf("code-minority group must keep the default, got %q", f)
	}
	// Invalid configured default falls back to chat.
	if f := ResolveFamily(memories("", ""), "bogus"); f != PromptFamilyChat {
		t.Fatalf("invalid default must fall back to chat, got %q", f)
	}
	// nil memories are ignored safely.
	if f := ResolveFamily([]*db.Memory{nil, nil}, "chat"); f != PromptFamilyChat {
		t.Fatalf("nil memories must not panic or switch family, got %q", f)
	}
}

func TestValidPromptFamily(t *testing.T) {
	for _, ok := range []string{PromptFamilyChat, PromptFamilyCode} {
		if !ValidPromptFamily(ok) {
			t.Errorf("%q must be valid", ok)
		}
	}
	for _, bad := range []string{"", "claude", "CODE"} {
		if ValidPromptFamily(bad) {
			t.Errorf("%q must be invalid", bad)
		}
	}
}
