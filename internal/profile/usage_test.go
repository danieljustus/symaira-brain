package profile

import (
	"strings"
	"testing"
)

// TestLoad_UsageServerPins the usage server wiring: enabled flag parses,
// an explicit mode is rejected with a warning (usage has no modes), and
// an absent section defaults to disabled.
func TestLoad_UsageServer(t *testing.T) {
	home := withHome(t)

	writeProfile(t, home, "usage-on", `[profile]
name = "usage-on"

[servers.usage]
enabled = true
`)
	p, err := Load("usage-on")
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if !p.Servers.Usage.Enabled {
		t.Error("usage enabled = false, want true")
	}
	if p.Server(ServerUsage).Enabled != true {
		t.Error("Server(usage).Enabled = false, want true")
	}

	writeProfile(t, home, "usage-off", `[profile]
name = "usage-off"
`)
	p2, err := Load("usage-off")
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if p2.Servers.Usage.Enabled {
		t.Error("usage enabled = true, want default false")
	}

	writeProfile(t, home, "usage-mode", `[profile]
name = "usage-mode"

[servers.usage]
enabled = true
mode = "full"
`)
	p3, err := Load("usage-mode")
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if p3.Servers.Usage.Mode != "" {
		t.Errorf("usage mode = %q, want ignored", p3.Servers.Usage.Mode)
	}
	found := false
	for _, w := range p3.Warnings {
		if strings.Contains(w, "usage") && strings.Contains(w, "mode") {
			found = true
		}
	}
	if !found {
		t.Errorf("Warnings = %v, want a mode-ignored warning for usage", p3.Warnings)
	}
}

// TestLoad_UsageToolsAllowDeny pins tools_allow/tools_deny passthrough for
// the usage server.
func TestLoad_UsageToolsAllowDeny(t *testing.T) {
	home := withHome(t)
	writeProfile(t, home, "usage-tools", `[profile]
name = "usage-tools"

[servers.usage]
enabled = true
tools_deny = ["get_ai_usage"]
`)
	p, err := Load("usage-tools")
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if len(p.Servers.Usage.ToolsDeny) != 1 || p.Servers.Usage.ToolsDeny[0] != "get_ai_usage" {
		t.Errorf("usage tools_deny = %v, want [get_ai_usage]", p.Servers.Usage.ToolsDeny)
	}
}
