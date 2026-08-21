package capability

import "testing"

func TestDenyControlPlane(t *testing.T) {
	tests := []struct {
		name   string
		target string
		want   bool
	}{
		{"config mutation", "symguard:config:set", true},
		{"grant management", "symguard:grant:revoke", true},
		{"policy mutation", "symguard:policy:set", true},
		{"token minting", "symguard:token:issue", true},
		{"audit tampering", "symguard:audit:truncate", true},
		{"identity management", "symguard:identity:impersonate", true},
		{"capability management", "symguard:capability:purge", true},
		{"bare symguard prefix is not control plane", "symguard:shell", false},
		{"regular tool name", "read_file", false},
		{"capability name", "shell", false},
		{"empty target", "", false},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := DenyControlPlane(tt.target); got != tt.want {
				t.Errorf("DenyControlPlane(%q) = %v, want %v", tt.target, got, tt.want)
			}
		})
	}
}

func TestInScope(t *testing.T) {
	tests := []struct {
		name   string
		scope  []string
		target string
		want   bool
	}{
		{"exact match", []string{"shell", "network"}, "shell", true},
		{"wildcard grants everything else", []string{"*"}, "read_secret", true},
		{"wildcard never grants control plane", []string{"*"}, "symguard:config:set", false},
		{"explicit control-plane entry still denied", []string{"symguard:config:set"}, "symguard:config:set", false},
		{"empty scope grants nothing", nil, "read_public", false},
		{"unlisted target denied", []string{"read_public"}, "shell", false},
		{"empty target denied", []string{"*"}, "", false},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := InScope(tt.scope, tt.target); got != tt.want {
				t.Errorf("InScope(%v, %q) = %v, want %v", tt.scope, tt.target, got, tt.want)
			}
		})
	}
}
