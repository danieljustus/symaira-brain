package security

import (
	"testing"
)

func TestRoleCanWrite(t *testing.T) {
	tests := []struct {
		name string
		role Role
		want bool
	}{
		{"read rejects writes", RoleRead, false},
		{"readwrite accepts writes", RoleReadWrite, true},
		{"admin accepts writes", RoleAdmin, true},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := tt.role.CanWrite(); got != tt.want {
				t.Errorf("Role(%q).CanWrite() = %v, want %v", tt.role, got, tt.want)
			}
		})
	}
}

func TestRoleCanAdmin(t *testing.T) {
	tests := []struct {
		name string
		role Role
		want bool
	}{
		{"read rejects admin", RoleRead, false},
		{"readwrite rejects admin", RoleReadWrite, false},
		{"admin accepts admin", RoleAdmin, true},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := tt.role.CanAdmin(); got != tt.want {
				t.Errorf("Role(%q).CanAdmin() = %v, want %v", tt.role, got, tt.want)
			}
		})
	}
}

func TestParseRole(t *testing.T) {
	tests := []struct {
		name string
		s    string
		want Role
	}{
		{"parses read", "read", RoleRead},
		{"parses readwrite", "readwrite", RoleReadWrite},
		{"parses admin", "admin", RoleAdmin},
		{"defaults unknown to read-only", "superadmin", RoleRead},
		{"defaults empty string to read-only", "", RoleRead},
		{"defaults gibberish to read-only", "foobar123", RoleRead},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := ParseRole(tt.s); got != tt.want {
				t.Errorf("ParseRole(%q) = %q, want %q", tt.s, got, tt.want)
			}
		})
	}
}
