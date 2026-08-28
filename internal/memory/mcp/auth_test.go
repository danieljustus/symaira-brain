package mcp

import (
	"net/http/httptest"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/memory/security"
)

func TestResolveProfileForRoleRequireProfileMatrix(t *testing.T) {
	tests := []struct {
		name           string
		requireProfile bool
		role           security.Role
		subject        string
		wantOK         bool
	}{
		{name: "permissive read unknown subject", role: security.RoleRead, subject: "unregistered", wantOK: true},
		{name: "permissive readwrite unknown subject", role: security.RoleReadWrite, subject: "unregistered", wantOK: true},
		{name: "required read unknown subject", requireProfile: true, role: security.RoleRead, subject: "unregistered", wantOK: false},
		{name: "required readwrite unknown subject", requireProfile: true, role: security.RoleReadWrite, subject: "unregistered", wantOK: false},
		{name: "permissive read empty subject", role: security.RoleRead, subject: "", wantOK: true},
		{name: "permissive readwrite empty subject", role: security.RoleReadWrite, subject: "", wantOK: true},
		{name: "required read empty subject", requireProfile: true, role: security.RoleRead, subject: "", wantOK: false},
		{name: "required readwrite empty subject", requireProfile: true, role: security.RoleReadWrite, subject: "", wantOK: false},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			database := helperDB(t)
			recorder := httptest.NewRecorder()
			payload := &security.JWTPayload{Subject: tc.subject}

			profile, ok := resolveProfileForRole(recorder, payload, nil, database, tc.requireProfile, tc.role)
			if ok != tc.wantOK {
				t.Fatalf("resolveProfileForRole ok = %v, want %v (status %d)", ok, tc.wantOK, recorder.Code)
			}
			if profile != nil {
				t.Fatalf("unregistered subject returned profile %+v", profile)
			}
			if !tc.wantOK && recorder.Code != 403 {
				t.Fatalf("denied request status = %d, want 403", recorder.Code)
			}
		})
	}
}
