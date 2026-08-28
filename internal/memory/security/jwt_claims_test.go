package security

import (
	"encoding/json"
	"strings"
	"testing"
	"time"
)

func tokenWithClaims(t *testing.T, provider *JWTProvider, subject, issuer string) string {
	t.Helper()
	token, err := provider.GenerateToken("test-subject", time.Hour)
	if err != nil {
		t.Fatalf("GenerateToken: %v", err)
	}
	parts := strings.Split(token, ".")
	if len(parts) != 3 {
		t.Fatalf("generated token has %d parts", len(parts))
	}

	payloadBytes, err := base64URLDecode(parts[1])
	if err != nil {
		t.Fatalf("decode generated payload: %v", err)
	}
	var payload JWTPayload
	if err := json.Unmarshal(payloadBytes, &payload); err != nil {
		t.Fatalf("unmarshal generated payload: %v", err)
	}
	payload.Subject = subject
	payload.Issuer = issuer
	payloadBytes, err = json.Marshal(payload)
	if err != nil {
		t.Fatalf("marshal modified payload: %v", err)
	}
	unsigned := parts[0] + "." + base64URL(payloadBytes)
	return unsigned + "." + provider.sign(unsigned)
}

func TestVerifyTokenRejectsEmptySubjectAndIssuerMismatch(t *testing.T) {
	provider := jwtProviderWithSecret(t, "claims-validation-test-secret")

	tests := []struct {
		name    string
		subject string
		issuer  string
		wantErr string
	}{
		{
			name:    "empty subject",
			subject: "",
			issuer:  "symaira-memory",
			wantErr: "empty subject",
		},
		{
			name:    "whitespace subject",
			subject: "  \t",
			issuer:  "symaira-memory",
			wantErr: "empty subject",
		},
		{
			name:    "issuer mismatch",
			subject: "agent",
			issuer:  "unexpected-issuer",
			wantErr: "invalid issuer",
		},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			token := tokenWithClaims(t, provider, tc.subject, tc.issuer)
			if _, err := provider.VerifyToken(token); err == nil || !strings.Contains(err.Error(), tc.wantErr) {
				t.Fatalf("VerifyToken error = %v, want error containing %q", err, tc.wantErr)
			}
		})
	}
}
