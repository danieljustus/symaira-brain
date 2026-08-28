package mcp

import (
	"context"
	"log/slog"
	"net/http"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
)

type contextKey string

const payloadKey contextKey = "jwt_payload"

func contextWithPayload(ctx context.Context, p *security.JWTPayload) context.Context {
	return context.WithValue(ctx, payloadKey, p)
}

func payloadFromContext(ctx context.Context) *security.JWTPayload {
	p, _ := ctx.Value(payloadKey).(*security.JWTPayload)
	return p
}

// AuthMiddleware handles JWT authentication and role-based access control.
type AuthMiddleware struct {
	jwts           *security.JWTProvider
	db             *db.DB
	profile        *db.Profile
	requireProfile bool
}

// NewAuthMiddleware creates an auth middleware with the given dependencies.
// requireProfile controls whether JWT subjects without a stored profile are
// denied write access (fail closed) or merely logged (permissive default).
func NewAuthMiddleware(jwts *security.JWTProvider, database *db.DB, requireProfile bool) *AuthMiddleware {
	return &AuthMiddleware{jwts: jwts, db: database, requireProfile: requireProfile}
}

// resolveProfileForRole looks up the profile for payload.Subject in database, falling back to
// override when set. It fails closed on a DB lookup error (denies rather than granting default
// access), and — when requireProfile is set — denies access to subjects with no stored profile
// at all (fail closed). In the permissive default it grants access to unknown subjects but logs a
// warning, since role enforcement is otherwise silently bypassable for any valid token whose
// subject has no matching profile.
func resolveProfileForRole(w http.ResponseWriter, payload *security.JWTPayload, override *db.Profile, database *db.DB, requireProfile bool, minRole security.Role) (*db.Profile, bool) {
	if override != nil {
		return override, true
	}

	if payload == nil || strings.TrimSpace(payload.Subject) == "" {
		if requireProfile {
			writeJSONError(w, http.StatusForbidden, CodeForbidden, "insufficient permissions: no profile registered for subject", nil)
			return nil, false
		}
		return nil, true
	}
	if database == nil {
		if requireProfile {
			writeJSONError(w, http.StatusForbidden, CodeForbidden, "failed to verify permissions", nil)
			return nil, false
		}
		return nil, true
	}

	p, err := database.GetProfileByName(payload.Subject)
	if err != nil {
		writeJSONError(w, http.StatusForbidden, CodeForbidden, "failed to verify permissions", err)
		return nil, false
	}
	if p == nil {
		if requireProfile {
			writeJSONError(w, http.StatusForbidden, CodeForbidden, "insufficient permissions: no profile registered for subject", nil)
			return nil, false
		}
		slog.Warn("JWT subject has no matching profile; granting default access", "subject", payload.Subject)
	}
	return p, true
}

// SetProfile sets the active profile for role-based access control.
func (a *AuthMiddleware) SetProfile(p *db.Profile) {
	a.profile = p
}

// RequireAuth returns a middleware that requires a valid JWT token.
// If jwts is nil, authentication is skipped (open access).
func (a *AuthMiddleware) RequireAuth(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if a.jwts == nil {
			next.ServeHTTP(w, r)
			return
		}
		auth := r.Header.Get("Authorization")
		if !strings.HasPrefix(auth, "Bearer ") {
			writeJSONError(w, http.StatusUnauthorized, CodeForbidden, "missing or invalid Authorization header", nil)
			return
		}
		token := strings.TrimPrefix(auth, "Bearer ")
		payload, err := a.jwts.VerifyToken(token)
		if err != nil {
			writeJSONError(w, http.StatusUnauthorized, CodeForbidden, "invalid or expired token", err)
			return
		}
		ctx := contextWithPayload(r.Context(), payload)
		next.ServeHTTP(w, r.WithContext(ctx))
	})
}

// RequireRole returns a middleware that requires a valid JWT with at least the given role.
// It must be wrapped inside RequireAuth: RequireAuth only calls next with a nil payload in
// context when jwts is nil (auth disabled, open access) — every genuinely unauthenticated
// request is already rejected before reaching here. So a nil payload at this point means
// "skip the role check", not "deny".
func (a *AuthMiddleware) RequireRole(minRole security.Role) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			payload := payloadFromContext(r.Context())
			if payload == nil {
				next.ServeHTTP(w, r)
				return
			}

			profile, ok := resolveProfileForRole(w, payload, a.profile, a.db, a.requireProfile, minRole)
			if !ok {
				return
			}

			if profile != nil {
				if !security.ParseRole(profile.Role).CanWrite() && minRole == security.RoleReadWrite {
					writeJSONError(w, http.StatusForbidden, CodeForbidden, "insufficient permissions: read-only profile", nil)
					return
				}
			}

			next.ServeHTTP(w, r)
		})
	}
}
