package db

import (
	"os"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
)

func openTestDB(t *testing.T) *DB {
	t.Helper()
	tempDir, err := os.MkdirTemp("", "symmemory-token-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	t.Cleanup(func() { os.RemoveAll(tempDir) })

	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	t.Cleanup(func() { os.Setenv("HOME", oldHome) })

	database, err := Open(config.Defaults())
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	t.Cleanup(func() { _ = database.Close() })
	return database
}

func TestRevokeTokenAndIsTokenRevoked(t *testing.T) {
	database := openTestDB(t)

	jti := "test-jti-abc123"

	// Before revocation, token should not be revoked
	revoked, err := database.IsTokenRevoked(jti)
	if err != nil {
		t.Fatalf("IsTokenRevoked failed: %v", err)
	}
	if revoked {
		t.Error("expected token to not be revoked before RevokeToken")
	}

	// Revoke the token
	if err := database.RevokeToken(jti); err != nil {
		t.Fatalf("RevokeToken failed: %v", err)
	}

	// After revocation, token should be revoked
	revoked, err = database.IsTokenRevoked(jti)
	if err != nil {
		t.Fatalf("IsTokenRevoked failed after revocation: %v", err)
	}
	if !revoked {
		t.Error("expected token to be revoked after RevokeToken")
	}
}

func TestIsTokenRevokedNotRevoked(t *testing.T) {
	database := openTestDB(t)

	revoked, err := database.IsTokenRevoked("nonexistent-jti")
	if err != nil {
		t.Fatalf("IsTokenRevoked failed: %v", err)
	}
	if revoked {
		t.Error("expected non-revoked token to return false")
	}
}

func TestRevokeTokenIdempotent(t *testing.T) {
	database := openTestDB(t)

	jti := "idempotent-jti"

	// Revoke twice should not error (INSERT OR IGNORE)
	if err := database.RevokeToken(jti); err != nil {
		t.Fatalf("first RevokeToken failed: %v", err)
	}
	if err := database.RevokeToken(jti); err != nil {
		t.Fatalf("second RevokeToken should not error (idempotent): %v", err)
	}

	revoked, err := database.IsTokenRevoked(jti)
	if err != nil {
		t.Fatalf("IsTokenRevoked failed: %v", err)
	}
	if !revoked {
		t.Error("expected token to be revoked after double revocation")
	}
}

func TestRevokeTokenMultipleJTIs(t *testing.T) {
	database := openTestDB(t)

	jtis := []string{"jti-1", "jti-2", "jti-3"}
	for _, jti := range jtis {
		if err := database.RevokeToken(jti); err != nil {
			t.Fatalf("RevokeToken(%s) failed: %v", jti, err)
		}
	}

	// All should be revoked
	for _, jti := range jtis {
		revoked, err := database.IsTokenRevoked(jti)
		if err != nil {
			t.Fatalf("IsTokenRevoked(%s) failed: %v", jti, err)
		}
		if !revoked {
			t.Errorf("expected %s to be revoked", jti)
		}
	}

	// A different JTI should not be revoked
	revoked, err := database.IsTokenRevoked("jti-other")
	if err != nil {
		t.Fatalf("IsTokenRevoked failed: %v", err)
	}
	if revoked {
		t.Error("expected unrevoked JTI to return false")
	}
}

func TestJwtRevocationsTableSchema(t *testing.T) {
	database := openTestDB(t)

	// Verify the jwt_revocations table exists and has the expected columns
	var name string
	err := database.conn.QueryRow(
		"SELECT name FROM sqlite_master WHERE type='table' AND name='jwt_revocations'",
	).Scan(&name)
	if err != nil {
		t.Fatalf("jwt_revocations table not found: %v", err)
	}
	if name != "jwt_revocations" {
		t.Errorf("expected table name 'jwt_revocations', got '%s'", name)
	}

	// Verify the table has jti and revoked_at columns by inserting and reading back
	testJTI := "schema-check-jti"
	if err := database.RevokeToken(testJTI); err != nil {
		t.Fatalf("RevokeToken failed: %v", err)
	}

	var jti string
	var revokedAt string
	err = database.conn.QueryRow(
		"SELECT jti, revoked_at FROM jwt_revocations WHERE jti = ?", testJTI,
	).Scan(&jti, &revokedAt)
	if err != nil {
		t.Fatalf("failed to read from jwt_revocations: %v", err)
	}
	if jti != testJTI {
		t.Errorf("expected jti '%s', got '%s'", testJTI, jti)
	}
	if revokedAt == "" {
		t.Error("expected revoked_at to be set")
	}
}
