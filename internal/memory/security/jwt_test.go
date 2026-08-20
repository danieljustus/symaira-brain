package security

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

// testConfigWithSecret builds a Config whose JWT secret_path points at a
// temp file containing the given secret. The previous NewJWTProvider(secret
// string) signature was replaced by config injection in #65; tests that need
// a stable secret now route the value through the file system to mirror the
// production path.
func testConfigWithSecret(t *testing.T, secret string) *config.Config {
	t.Helper()
	dir := t.TempDir()
	secretPath := filepath.Join(dir, "jwt.test.secret")
	if err := os.WriteFile(secretPath, []byte(secret), 0600); err != nil {
		t.Fatalf("failed to write test secret: %v", err)
	}
	return &config.Config{
		JWT: config.JWTConfig{SecretPath: secretPath},
	}
}

func jwtProviderWithSecret(t *testing.T, secret string) *JWTProvider {
	t.Helper()
	provider, err := NewJWTProvider(testConfigWithSecret(t, secret), nil)
	if err != nil {
		t.Fatalf("failed to create jwt provider: %v", err)
	}
	return provider
}

func TestJWTGenerationAndVerification(t *testing.T) {
	secret := "my_custom_secure_test_signing_key_2026"
	provider := jwtProviderWithSecret(t, secret)

	subject := "test-agent"
	duration := 10 * time.Minute

	// Test Token Generation
	token, err := provider.GenerateToken(subject, duration)
	if err != nil {
		t.Fatalf("failed to generate token: %v", err)
	}

	if token == "" {
		t.Fatalf("generated token is empty")
	}

	// Test Token Verification
	payload, err := provider.VerifyToken(token)
	if err != nil {
		t.Fatalf("failed to verify valid token: %v", err)
	}

	if payload.Subject != subject {
		t.Errorf("expected subject '%s', got '%s'", subject, payload.Subject)
	}
	if payload.Issuer != "symaira-memory" {
		t.Errorf("expected issuer 'symaira-memory', got '%s'", payload.Issuer)
	}
	if payload.JWTID == "" {
		t.Errorf("expected non-empty jti in payload")
	}

	// Test Tampering Rejection (modify one character in signature)
	originalChar := token[len(token)-5]
	tamperedChar := "A"
	if originalChar == 'A' {
		tamperedChar = "B"
	}
	tamperedToken := token[:len(token)-5] + tamperedChar + token[len(token)-4:]
	_, err = provider.VerifyToken(tamperedToken)
	if err == nil {
		t.Errorf("verification should fail on tampered token")
	}

	// Test Expiration Verification
	expiredProvider := jwtProviderWithSecret(t, secret)
	expiredToken, err := expiredProvider.GenerateToken(subject, -5*time.Second) // expired 5s ago
	if err != nil {
		t.Fatalf("failed to generate expired token: %v", err)
	}

	_, err = expiredProvider.VerifyToken(expiredToken)
	if err == nil {
		t.Errorf("verification should fail on expired token")
	} else if !strings.Contains(err.Error(), "expired") {
		t.Errorf("expected expiration error message, got: %v", err)
	}
}

func TestJWTRevocation(t *testing.T) {
	provider := jwtProviderWithSecret(t, "revocation-test-secret")

	token, err := provider.GenerateToken("agent", 10*time.Minute)
	if err != nil {
		t.Fatalf("failed to generate token: %v", err)
	}

	payload, err := provider.VerifyToken(token)
	if err != nil {
		t.Fatalf("token should be valid before revocation: %v", err)
	}

	if err := provider.RevokeToken(payload.JWTID); err != nil {
		t.Fatalf("RevokeToken should succeed with in-memory store: %v", err)
	}

	_, err = provider.VerifyToken(token)
	if err == nil {
		t.Errorf("verification should fail after revocation")
	} else if !strings.Contains(err.Error(), "revoked") {
		t.Errorf("expected revocation error, got: %v", err)
	}
}

// failingRevocationStore is a RevocationStore whose persistence always fails,
// used to prove RevokeToken propagates the store error instead of silently
// swallowing it while still applying the in-memory revocation.
type failingRevocationStore struct{}

func (failingRevocationStore) RevokeToken(jti string) error {
	return errors.New("test store failure: disk full")
}

func (failingRevocationStore) IsTokenRevoked(jti string) (bool, error) {
	return false, nil
}

func TestJWTRevocationStoreErrorPropagates(t *testing.T) {
	provider := jwtProviderWithStore(t, failingRevocationStore{}, t.TempDir())

	token, err := provider.GenerateToken("agent", 10*time.Minute)
	if err != nil {
		t.Fatalf("failed to generate token: %v", err)
	}
	payload, err := provider.VerifyToken(token)
	if err != nil {
		t.Fatalf("token should be valid before revocation: %v", err)
	}

	err = provider.RevokeToken(payload.JWTID)
	if err == nil {
		t.Fatal("expected RevokeToken to propagate the persistence error")
	}
	if !strings.Contains(err.Error(), "failed to persist token revocation") {
		t.Errorf("expected persistence failure error, got: %v", err)
	}

	// The in-memory fallback must still have taken effect.
	_, err = provider.VerifyToken(token)
	if err == nil {
		t.Errorf("verification should fail after revocation even when persistence fails")
	} else if !strings.Contains(err.Error(), "revoked") {
		t.Errorf("expected revocation error, got: %v", err)
	}
}

func TestJWTRevocationPersistsAcrossProviders(t *testing.T) {
	database, dir := openTestDB(t)

	providerA := jwtProviderWithStore(t, database, dir)
	token, err := providerA.GenerateToken("agent", 10*time.Minute)
	if err != nil {
		t.Fatalf("failed to generate token: %v", err)
	}
	payload, err := providerA.VerifyToken(token)
	if err != nil {
		t.Fatalf("token should be valid before revocation: %v", err)
	}

	if err := providerA.RevokeToken(payload.JWTID); err != nil {
		t.Fatalf("RevokeToken with persistent store failed: %v", err)
	}

	// A fresh provider backed by the same database must reject the token.
	providerB := jwtProviderWithStore(t, database, dir)
	_, err = providerB.VerifyToken(token)
	if err == nil {
		t.Errorf("verification should fail after persistent revocation")
	} else if !strings.Contains(err.Error(), "revoked") {
		t.Errorf("expected revocation error, got: %v", err)
	}
}

func TestExtractJTI(t *testing.T) {
	provider := jwtProviderWithSecret(t, "extract-jti-test-secret")
	token, err := provider.GenerateToken("agent", 10*time.Minute)
	if err != nil {
		t.Fatalf("failed to generate token: %v", err)
	}
	payload, err := provider.VerifyToken(token)
	if err != nil {
		t.Fatalf("failed to verify token: %v", err)
	}

	jti, err := ExtractJTI(token)
	if err != nil {
		t.Fatalf("ExtractJTI failed on a valid token: %v", err)
	}
	if jti != payload.JWTID {
		t.Errorf("expected jti %q, got %q", payload.JWTID, jti)
	}

	for _, bad := range []string{"", "not-a-jwt", "only.two.parts", "a.b.c.d", "abc..def"} {
		if _, err := ExtractJTI(bad); err == nil {
			t.Errorf("expected ExtractJTI to reject %q", bad)
		}
	}
}

func TestJWTKeyRotation(t *testing.T) {
	provider := jwtProviderWithSecret(t, "old-secret-v1")

	oldToken, err := provider.GenerateToken("agent", 10*time.Minute)
	if err != nil {
		t.Fatalf("failed to generate token with old key: %v", err)
	}

	provider.RotateSecret("new-secret-v2")

	// Old token should still verify (grace period)
	_, err = provider.VerifyToken(oldToken)
	if err != nil {
		t.Errorf("old token should still be valid after rotation: %v", err)
	}

	// New token should verify
	newToken, err := provider.GenerateToken("agent", 10*time.Minute)
	if err != nil {
		t.Fatalf("failed to generate token with new key: %v", err)
	}
	_, err = provider.VerifyToken(newToken)
	if err != nil {
		t.Errorf("new token should verify with rotated key: %v", err)
	}

	// Token signed with unrelated key should fail
	forgedProvider := jwtProviderWithSecret(t, "unrelated-key")
	forgedToken, _ := forgedProvider.GenerateToken("agent", 10*time.Minute)
	_, err = provider.VerifyToken(forgedToken)
	if err == nil {
		t.Errorf("token from unrelated key should fail verification")
	}
}

func TestRotateSecretPersistsFallbackToDisk(t *testing.T) {
	dir := t.TempDir()
	secretPath := filepath.Join(dir, "jwt.test.secret")
	cfg := &config.Config{
		JWT: config.JWTConfig{SecretPath: secretPath},
	}

	provider, err := NewJWTProvider(cfg, nil)
	if err != nil {
		t.Fatalf("failed to create provider: %v", err)
	}

	oldSecret := string(provider.secret)
	oldToken, err := provider.GenerateToken("agent", 10*time.Minute)
	if err != nil {
		t.Fatalf("failed to generate token: %v", err)
	}

	provider.RotateSecret("new-secret-v2")

	fallbackPath := strings.TrimSuffix(secretPath, filepath.Ext(secretPath)) + ".secrets"
	data, err := os.ReadFile(fallbackPath)
	if err != nil {
		t.Fatalf("fallback secrets file not created: %v", err)
	}

	// Data should be encrypted (not plaintext JSON)
	if strings.Contains(string(data), oldSecret) {
		t.Error("fallback file should be encrypted, not contain plaintext secret")
	}

	// Verify we can decrypt and the old secret is present
	loaded, loadErr := loadFallbackSecrets(cfg, provider.secret)
	if loadErr != nil {
		t.Fatalf("failed to load fallback secrets: %v", loadErr)
	}
	found := false
	for _, e := range loaded {
		if e.Secret == oldSecret {
			found = true
			break
		}
	}
	if !found {
		t.Errorf("fallback secrets should contain old secret %q, loaded: %+v", oldSecret, loaded)
	}

	_, err = provider.VerifyToken(oldToken)
	if err != nil {
		t.Errorf("old token should still verify after rotation: %v", err)
	}
}

func TestLoadFallbackSecretsOnStartup(t *testing.T) {
	dir := t.TempDir()
	secretPath := filepath.Join(dir, "jwt.test.secret")
	if err := os.WriteFile(secretPath, []byte("primary-secret"), 0600); err != nil {
		t.Fatalf("failed to write secret: %v", err)
	}

	fallbackPath := strings.TrimSuffix(secretPath, filepath.Ext(secretPath)) + ".secrets"
	fallbackJSON := `[{"secret":"old-fallback-secret","expires_at":"2099-12-31T23:59:59Z"}]`
	if err := os.WriteFile(fallbackPath, []byte(fallbackJSON), 0600); err != nil {
		t.Fatalf("failed to write fallback: %v", err)
	}

	cfg := &config.Config{
		JWT: config.JWTConfig{SecretPath: secretPath},
	}

	provider, err := NewJWTProvider(cfg, nil)
	if err != nil {
		t.Fatalf("failed to create provider: %v", err)
	}

	if len(provider.secrets) != 1 {
		t.Fatalf("expected 1 fallback secret, got %d", len(provider.secrets))
	}
	if string(provider.secrets[0]) != "old-fallback-secret" {
		t.Errorf("expected fallback secret 'old-fallback-secret', got %q", string(provider.secrets[0]))
	}

	oldProvider := &JWTProvider{secret: []byte("old-fallback-secret")}
	oldToken, _ := oldProvider.GenerateToken("agent", 10*time.Minute)

	_, err = provider.VerifyToken(oldToken)
	if err != nil {
		t.Errorf("token signed with loaded fallback should verify: %v", err)
	}
}

func TestGracePeriodExpirationPurgesOldSecrets(t *testing.T) {
	dir := t.TempDir()
	secretPath := filepath.Join(dir, "jwt.test.secret")
	if err := os.WriteFile(secretPath, []byte("primary-secret"), 0600); err != nil {
		t.Fatalf("failed to write secret: %v", err)
	}

	fallbackPath := strings.TrimSuffix(secretPath, filepath.Ext(secretPath)) + ".secrets"
	expiredJSON := `[{"secret":"expired-secret","expires_at":"2020-01-01T00:00:00Z"}]`
	if err := os.WriteFile(fallbackPath, []byte(expiredJSON), 0600); err != nil {
		t.Fatalf("failed to write fallback: %v", err)
	}

	cfg := &config.Config{
		JWT: config.JWTConfig{SecretPath: secretPath},
	}

	provider, err := NewJWTProvider(cfg, nil)
	if err != nil {
		t.Fatalf("failed to create provider: %v", err)
	}

	if len(provider.secrets) != 0 {
		t.Errorf("expired fallback should be purged, got %d secrets", len(provider.secrets))
	}

	data, err := os.ReadFile(fallbackPath)
	if err != nil {
		t.Fatalf("failed to read fallback file: %v", err)
	}
	if strings.Contains(string(data), "expired-secret") {
		t.Errorf("expired secret should be purged from disk, got: %s", string(data))
	}
}

func TestRotationPurgesExpiredFallbacks(t *testing.T) {
	dir := t.TempDir()
	secretPath := filepath.Join(dir, "jwt.test.secret")
	cfg := &config.Config{
		JWT: config.JWTConfig{SecretPath: secretPath},
	}

	provider, err := NewJWTProvider(cfg, nil)
	if err != nil {
		t.Fatalf("failed to create provider: %v", err)
	}
	provider.gracePeriod = 1 * time.Millisecond

	provider.RotateSecret("second-secret")

	time.Sleep(5 * time.Millisecond)

	provider.RotateSecret("third-secret")

	fallbackPath := strings.TrimSuffix(secretPath, filepath.Ext(secretPath)) + ".secrets"
	data, err := os.ReadFile(fallbackPath)
	if err != nil {
		t.Fatalf("failed to read fallback file: %v", err)
	}

	if strings.Contains(string(data), string([]byte("first"))) {
		t.Logf("fallback file contents: %s", string(data))
	}

	if len(provider.secrets) > 1 {
		t.Errorf("expected at most 1 fallback after purge, got %d", len(provider.secrets))
	}
}

// openTestDB creates an isolated SQLite database in a temp directory that
// mirrors the production XDG layout. The caller must defer db.Close().
func openTestDB(t *testing.T) (*db.DB, string) {
	t.Helper()
	tempDir, err := os.MkdirTemp("", "symmemory-jwt-revoke-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	setTestHome(t, tempDir)
	t.Cleanup(func() {
		os.RemoveAll(tempDir)
	})

	database, err := db.Open(config.Defaults())
	if err != nil {
		t.Fatalf("failed to open test database: %v", err)
	}
	t.Cleanup(func() { _ = database.Close() })

	return database, tempDir
}

// jwtProviderWithStore creates a JWTProvider backed by the given revocation
// store and a stable secret written to dir.
func jwtProviderWithStore(t *testing.T, store RevocationStore, dir string) *JWTProvider {
	t.Helper()
	secretPath := filepath.Join(dir, "jwt.test.secret")
	if err := os.WriteFile(secretPath, []byte("revocation-persistence-secret"), 0600); err != nil {
		t.Fatalf("failed to write test secret: %v", err)
	}
	cfg := &config.Config{
		JWT: config.JWTConfig{SecretPath: secretPath},
	}
	provider, err := NewJWTProvider(cfg, store)
	if err != nil {
		t.Fatalf("failed to create jwt provider: %v", err)
	}
	return provider
}

func TestRevocationStoreIsTokenRevoked(t *testing.T) {
	database, _ := openTestDB(t)

	revoked, err := database.IsTokenRevoked("nonexistent-jti")
	if err != nil {
		t.Fatalf("IsTokenRevoked failed: %v", err)
	}
	if revoked {
		t.Error("expected IsTokenRevoked to return false for unknown JTI")
	}

	if err := database.RevokeToken("jti-abc"); err != nil {
		t.Fatalf("RevokeToken failed: %v", err)
	}

	revoked, err = database.IsTokenRevoked("jti-abc")
	if err != nil {
		t.Fatalf("IsTokenRevoked failed: %v", err)
	}
	if !revoked {
		t.Error("expected IsTokenRevoked to return true after RevokeToken")
	}
}

func TestRevocationStoreIdempotent(t *testing.T) {
	database, _ := openTestDB(t)

	if err := database.RevokeToken("jti-dup"); err != nil {
		t.Fatalf("first RevokeToken failed: %v", err)
	}
	if err := database.RevokeToken("jti-dup"); err != nil {
		t.Fatalf("second RevokeToken (idempotent) failed: %v", err)
	}

	revoked, err := database.IsTokenRevoked("jti-dup")
	if err != nil {
		t.Fatalf("IsTokenRevoked failed: %v", err)
	}
	if !revoked {
		t.Error("expected JTI to be revoked after double insert")
	}
}

func TestRevokedJTIPersistedAcrossFreshProvider(t *testing.T) {
	database, tempDir := openTestDB(t)

	provider1 := jwtProviderWithStore(t, database, tempDir)

	token, err := provider1.GenerateToken("test-agent", 10*time.Minute)
	if err != nil {
		t.Fatalf("GenerateToken failed: %v", err)
	}

	payload, err := provider1.VerifyToken(token)
	if err != nil {
		t.Fatalf("token should be valid before revocation: %v", err)
	}

	_ = provider1.RevokeToken(payload.JWTID)

	_, err = provider1.VerifyToken(token)
	if err == nil {
		t.Fatal("provider1 should reject revoked token")
	}

	provider2 := jwtProviderWithStore(t, database, tempDir)

	_, err = provider2.VerifyToken(token)
	if err == nil {
		t.Fatal("fresh provider should reject persisted revoked token")
	}
	if !strings.Contains(err.Error(), "revoked") {
		t.Errorf("expected revocation error, got: %v", err)
	}
}

func TestNonRevokedTokenAcceptedByFreshProvider(t *testing.T) {
	database, tempDir := openTestDB(t)

	provider1 := jwtProviderWithStore(t, database, tempDir)

	token, err := provider1.GenerateToken("test-agent", 10*time.Minute)
	if err != nil {
		t.Fatalf("GenerateToken failed: %v", err)
	}

	if _, err := provider1.VerifyToken(token); err != nil {
		t.Fatalf("token should verify on provider1: %v", err)
	}

	provider2 := jwtProviderWithStore(t, database, tempDir)

	if _, err := provider2.VerifyToken(token); err != nil {
		t.Errorf("fresh provider should accept non-revoked token: %v", err)
	}
}

func TestMultipleRevocationsPersisted(t *testing.T) {
	database, tempDir := openTestDB(t)

	provider := jwtProviderWithStore(t, database, tempDir)

	tokens := make([]string, 3)
	jtis := make([]string, 3)
	for i := 0; i < 3; i++ {
		tok, err := provider.GenerateToken("agent", 10*time.Minute)
		if err != nil {
			t.Fatalf("GenerateToken %d failed: %v", i, err)
		}
		tokens[i] = tok

		p, err := provider.VerifyToken(tok)
		if err != nil {
			t.Fatalf("VerifyToken %d failed: %v", i, err)
		}
		jtis[i] = p.JWTID
	}

	_ = provider.RevokeToken(jtis[1])

	provider2 := jwtProviderWithStore(t, database, tempDir)

	for i, tok := range tokens {
		_, err := provider2.VerifyToken(tok)
		if i == 1 {
			if err == nil {
				t.Errorf("token %d should be rejected (revoked)", i)
			}
		} else {
			if err != nil {
				t.Errorf("token %d should be accepted: %v", i, err)
			}
		}
	}
}

// TestSecretResolutionFromFile verifies loading secret from cfg.JWT.SecretPath
func TestSecretResolutionFromFile(t *testing.T) {
	dir := t.TempDir()
	secretPath := filepath.Join(dir, "jwt.test.secret")
	expectedSecret := "file-loaded-secret-12345"
	if err := os.WriteFile(secretPath, []byte(expectedSecret+"\n"), 0600); err != nil {
		t.Fatalf("failed to write secret: %v", err)
	}

	cfg := &config.Config{
		JWT: config.JWTConfig{SecretPath: secretPath},
	}

	provider, err := NewJWTProvider(cfg, nil)
	if err != nil {
		t.Fatalf("failed to create provider: %v", err)
	}

	if string(provider.secret) != expectedSecret {
		t.Errorf("expected secret %q, got %q", expectedSecret, string(provider.secret))
	}

	token, err := provider.GenerateToken("test", 10*time.Minute)
	if err != nil {
		t.Fatalf("failed to generate token: %v", err)
	}

	payload, err := provider.VerifyToken(token)
	if err != nil {
		t.Fatalf("failed to verify token: %v", err)
	}
	if payload.Subject != "test" {
		t.Errorf("expected subject 'test', got %q", payload.Subject)
	}
}

// TestSecretAutoGeneration verifies auto-generating secret when none exists
func TestSecretAutoGeneration(t *testing.T) {
	dir := t.TempDir()
	secretPath := filepath.Join(dir, "subdir", "jwt.test.secret")

	cfg := &config.Config{
		JWT: config.JWTConfig{SecretPath: secretPath},
	}

	provider, err := NewJWTProvider(cfg, nil)
	if err != nil {
		t.Fatalf("failed to create provider: %v", err)
	}

	if len(provider.secret) == 0 {
		t.Fatal("expected auto-generated secret, got empty")
	}

	if len(provider.secret) != 64 {
		t.Errorf("expected 64-byte hex secret, got %d bytes", len(provider.secret))
	}

	data, err := os.ReadFile(secretPath)
	if err != nil {
		t.Fatalf("failed to read persisted secret: %v", err)
	}

	if string(data) != string(provider.secret)+"\n" {
		t.Errorf("persisted secret mismatch: file=%q, memory=%q", string(data), string(provider.secret))
	}

	token, err := provider.GenerateToken("test", 10*time.Minute)
	if err != nil {
		t.Fatalf("failed to generate token: %v", err)
	}

	_, err = provider.VerifyToken(token)
	if err != nil {
		t.Fatalf("failed to verify token with auto-generated secret: %v", err)
	}
}

// TestSecretResolutionFromEnv verifies JWT_SECRET_KEY env var resolution
func TestSecretResolutionFromEnv(t *testing.T) {
	dir := t.TempDir()
	secretPath := filepath.Join(dir, "jwt.test.secret")

	envSecret := "env-var-secret-67890"
	t.Setenv("JWT_SECRET_KEY", envSecret)

	cfg := &config.Config{
		JWT: config.JWTConfig{SecretPath: secretPath},
	}

	provider, err := NewJWTProvider(cfg, nil)
	if err != nil {
		t.Fatalf("failed to create provider: %v", err)
	}

	if string(provider.secret) != envSecret {
		t.Errorf("expected env secret %q, got %q", envSecret, string(provider.secret))
	}

	if _, err := os.Stat(secretPath); err == nil {
		t.Error("secret file should not be created when env var is set")
	}

	token, err := provider.GenerateToken("test", 10*time.Minute)
	if err != nil {
		t.Fatalf("failed to generate token: %v", err)
	}

	_, err = provider.VerifyToken(token)
	if err != nil {
		t.Fatalf("failed to verify token: %v", err)
	}
}

// TestFallbackSecretPersistenceAcrossRotation verifies fallback secrets persist
func TestFallbackSecretPersistenceAcrossRotation(t *testing.T) {
	dir := t.TempDir()
	secretPath := filepath.Join(dir, "jwt.test.secret")
	cfg := &config.Config{
		JWT: config.JWTConfig{SecretPath: secretPath},
	}

	provider, err := NewJWTProvider(cfg, nil)
	if err != nil {
		t.Fatalf("failed to create provider: %v", err)
	}

	firstSecret := string(provider.secret)
	firstToken, err := provider.GenerateToken("agent", 10*time.Minute)
	if err != nil {
		t.Fatalf("failed to generate first token: %v", err)
	}

	provider.RotateSecret("second-secret")
	secondToken, err := provider.GenerateToken("agent", 10*time.Minute)
	if err != nil {
		t.Fatalf("failed to generate second token: %v", err)
	}

	provider.RotateSecret("third-secret")
	thirdToken, err := provider.GenerateToken("agent", 10*time.Minute)
	if err != nil {
		t.Fatalf("failed to generate third token: %v", err)
	}

	fallbackPath := strings.TrimSuffix(secretPath, filepath.Ext(secretPath)) + ".secrets"
	data, err := os.ReadFile(fallbackPath)
	if err != nil {
		t.Fatalf("failed to read fallback file: %v", err)
	}

	// Data should be encrypted (not plaintext JSON)
	if strings.Contains(string(data), firstSecret) {
		t.Error("fallback file should be encrypted, not contain plaintext secret")
	}

	// Verify decrypted content has both rotated secrets
	loaded, loadErr := loadFallbackSecrets(cfg, provider.secret)
	if loadErr != nil {
		t.Fatalf("failed to load fallback secrets: %v", loadErr)
	}
	secretsFound := make(map[string]bool)
	for _, e := range loaded {
		secretsFound[e.Secret] = true
	}
	if !secretsFound[firstSecret] {
		t.Errorf("fallback should contain first secret %q, loaded: %+v", firstSecret, loaded)
	}
	if !secretsFound["second-secret"] {
		t.Errorf("fallback should contain second-secret, loaded: %+v", loaded)
	}

	if _, err := provider.VerifyToken(firstToken); err != nil {
		t.Errorf("first token should still verify: %v", err)
	}
	if _, err := provider.VerifyToken(secondToken); err != nil {
		t.Errorf("second token should still verify: %v", err)
	}
	if _, err := provider.VerifyToken(thirdToken); err != nil {
		t.Errorf("third token should verify: %v", err)
	}
}

// TestPruningExpiredFallbackSecrets verifies expired fallbacks are pruned
func TestPruningExpiredFallbackSecrets(t *testing.T) {
	dir := t.TempDir()
	secretPath := filepath.Join(dir, "jwt.test.secret")
	if err := os.WriteFile(secretPath, []byte("primary"), 0600); err != nil {
		t.Fatalf("failed to write secret: %v", err)
	}

	fallbackPath := strings.TrimSuffix(secretPath, filepath.Ext(secretPath)) + ".secrets"
	expiredJSON := `[
		{"secret":"expired-1","expires_at":"2020-01-01T00:00:00Z"},
		{"secret":"valid-1","expires_at":"2099-12-31T23:59:59Z"},
		{"secret":"expired-2","expires_at":"2021-01-01T00:00:00Z"}
	]`
	if err := os.WriteFile(fallbackPath, []byte(expiredJSON), 0600); err != nil {
		t.Fatalf("failed to write fallback: %v", err)
	}

	cfg := &config.Config{
		JWT: config.JWTConfig{SecretPath: secretPath},
	}

	provider, err := NewJWTProvider(cfg, nil)
	if err != nil {
		t.Fatalf("failed to create provider: %v", err)
	}

	if len(provider.secrets) != 1 {
		t.Errorf("expected 1 valid fallback, got %d", len(provider.secrets))
	}
	if string(provider.secrets[0]) != "valid-1" {
		t.Errorf("expected valid-1, got %q", string(provider.secrets[0]))
	}

	data, err := os.ReadFile(fallbackPath)
	if err != nil {
		t.Fatalf("failed to read fallback file: %v", err)
	}

	// Raw file should be encrypted after migration
	if strings.Contains(string(data), "expired-1") || strings.Contains(string(data), "valid-1") {
		t.Error("fallback file should be encrypted, not contain plaintext entries")
	}

	// Verify via decryption that expired entries are pruned
	loaded, loadErr := loadFallbackSecrets(cfg, provider.secret)
	if loadErr != nil {
		t.Fatalf("failed to load fallback secrets: %v", loadErr)
	}
	if len(loaded) != 1 || loaded[0].Secret != "valid-1" {
		t.Errorf("expected only valid-1 after pruning, got %+v", loaded)
	}
}

// TestMalformedFallbackJSON verifies error handling for malformed JSON
func TestMalformedFallbackJSON(t *testing.T) {
	dir := t.TempDir()
	secretPath := filepath.Join(dir, "jwt.test.secret")
	if err := os.WriteFile(secretPath, []byte("primary"), 0600); err != nil {
		t.Fatalf("failed to write secret: %v", err)
	}

	fallbackPath := strings.TrimSuffix(secretPath, filepath.Ext(secretPath)) + ".secrets"
	malformedJSON := `{"secret":"not-an-array","expires_at":"2099-12-31"}`
	if err := os.WriteFile(fallbackPath, []byte(malformedJSON), 0600); err != nil {
		t.Fatalf("failed to write malformed fallback: %v", err)
	}

	cfg := &config.Config{
		JWT: config.JWTConfig{SecretPath: secretPath},
	}

	provider, err := NewJWTProvider(cfg, nil)
	if err != nil {
		t.Fatalf("provider creation should succeed despite malformed JSON: %v", err)
	}

	if len(provider.secrets) != 0 {
		t.Errorf("expected 0 fallbacks after malformed JSON, got %d", len(provider.secrets))
	}

	token, err := provider.GenerateToken("test", 10*time.Minute)
	if err != nil {
		t.Fatalf("failed to generate token: %v", err)
	}

	_, err = provider.VerifyToken(token)
	if err != nil {
		t.Fatalf("token should still verify with primary secret: %v", err)
	}
}

// TestAddFallbackSecret verifies AddFallbackSecret behavior
func TestAddFallbackSecret(t *testing.T) {
	dir := t.TempDir()
	secretPath := filepath.Join(dir, "jwt.test.secret")
	cfg := &config.Config{
		JWT: config.JWTConfig{SecretPath: secretPath},
	}

	provider, err := NewJWTProvider(cfg, nil)
	if err != nil {
		t.Fatalf("failed to create provider: %v", err)
	}

	fallbackSecret := "manual-fallback-secret"
	provider.AddFallbackSecret(fallbackSecret)

	if len(provider.secrets) != 1 {
		t.Fatalf("expected 1 fallback secret, got %d", len(provider.secrets))
	}
	if string(provider.secrets[0]) != fallbackSecret {
		t.Errorf("expected fallback %q, got %q", fallbackSecret, string(provider.secrets[0]))
	}

	fallbackProvider := &JWTProvider{secret: []byte(fallbackSecret)}
	fallbackToken, err := fallbackProvider.GenerateToken("agent", 10*time.Minute)
	if err != nil {
		t.Fatalf("failed to generate fallback token: %v", err)
	}

	_, err = provider.VerifyToken(fallbackToken)
	if err != nil {
		t.Errorf("token signed with added fallback should verify: %v", err)
	}

	provider.AddFallbackSecret("another-fallback")
	if len(provider.secrets) != 2 {
		t.Errorf("expected 2 fallback secrets, got %d", len(provider.secrets))
	}
}

// TestLoadPersistedSecretDefaultPath verifies default path resolution
func TestLoadPersistedSecretDefaultPath(t *testing.T) {
	dir := t.TempDir()
	setTestHome(t, dir)

	// Clear XDG_CONFIG_HOME to test default behavior
	oldXDG := os.Getenv("XDG_CONFIG_HOME")
	os.Setenv("XDG_CONFIG_HOME", "")
	t.Cleanup(func() {
		os.Setenv("XDG_CONFIG_HOME", oldXDG)
	})

	configDir := filepath.Join(dir, ".config", "symmemory")
	if err := os.MkdirAll(configDir, 0700); err != nil {
		t.Fatalf("failed to create config dir: %v", err)
	}

	secretPath := filepath.Join(configDir, "jwt.secret")
	expectedSecret := "default-path-secret"
	if err := os.WriteFile(secretPath, []byte(expectedSecret), 0600); err != nil {
		t.Fatalf("failed to write secret: %v", err)
	}

	cfg := &config.Config{
		JWT: config.JWTConfig{},
	}

	secret, err := loadPersistedSecret(cfg)
	if err != nil {
		t.Fatalf("loadPersistedSecret failed: %v", err)
	}

	if secret != expectedSecret {
		t.Errorf("expected %q, got %q", expectedSecret, secret)
	}
}

// TestGenerateAndPersistSecretCreatesDirectory verifies directory creation
func TestGenerateAndPersistSecretCreatesDirectory(t *testing.T) {
	dir := t.TempDir()
	secretPath := filepath.Join(dir, "deep", "nested", "path", "jwt.secret")

	cfg := &config.Config{
		JWT: config.JWTConfig{SecretPath: secretPath},
	}

	secret, err := generateAndPersistSecret(cfg)
	if err != nil {
		t.Fatalf("generateAndPersistSecret failed: %v", err)
	}

	if len(secret) != 64 {
		t.Errorf("expected 64-byte hex secret, got %d bytes", len(secret))
	}

	data, err := os.ReadFile(secretPath)
	if err != nil {
		t.Fatalf("failed to read persisted secret: %v", err)
	}

	if string(data) != secret+"\n" {
		t.Errorf("persisted content mismatch")
	}

	info, err := os.Stat(filepath.Dir(secretPath))
	if err != nil {
		t.Fatalf("failed to stat directory: %v", err)
	}
	if runtime.GOOS != "windows" && info.Mode().Perm() != 0700 {
		t.Errorf("expected directory permissions 0700, got %o", info.Mode().Perm())
	}
}

// TestFallbackSecretsPathResolution verifies path computation
func TestFallbackSecretsPathResolution(t *testing.T) {
	tests := []struct {
		name       string
		secretPath string
		expected   string
	}{
		{
			name:       "with extension",
			secretPath: "/tmp/jwt.secret",
			expected:   "/tmp/jwt.secrets",
		},
		{
			name:       "without extension",
			secretPath: "/tmp/jwt",
			expected:   "/tmp/jwt.secrets",
		},
		{
			name:       "nested path",
			secretPath: "/home/user/.config/symmemory/jwt.key",
			expected:   "/home/user/.config/symmemory/jwt.secrets",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			cfg := &config.Config{
				JWT: config.JWTConfig{SecretPath: tt.secretPath},
			}
			result := fallbackSecretsPath(cfg)
			if result != tt.expected {
				t.Errorf("expected %q, got %q", tt.expected, result)
			}
		})
	}
}

// TestLoadFallbackSecretsFileNotFound verifies handling of missing file
func TestLoadFallbackSecretsFileNotFound(t *testing.T) {
	dir := t.TempDir()
	secretPath := filepath.Join(dir, "nonexistent.secret")

	cfg := &config.Config{
		JWT: config.JWTConfig{SecretPath: secretPath},
	}

	entries, err := loadFallbackSecrets(cfg, []byte("test-primary-secret"))
	if err != nil {
		t.Fatalf("loadFallbackSecrets should not error on missing file: %v", err)
	}
	if len(entries) != 0 {
		t.Errorf("expected 0 entries for missing file, got %d", len(entries))
	}
}

// TestPersistFallbackSecretsCreatesDirectory verifies directory creation
func TestPersistFallbackSecretsCreatesDirectory(t *testing.T) {
	dir := t.TempDir()
	secretPath := filepath.Join(dir, "deep", "nested", "jwt.secret")

	cfg := &config.Config{
		JWT: config.JWTConfig{SecretPath: secretPath},
	}

	entries := []fallbackEntry{
		{Secret: "test-secret", ExpiresAt: time.Now().Add(24 * time.Hour)},
	}

	primarySecret := []byte("test-primary-secret")
	err := persistFallbackSecrets(cfg, entries, primarySecret)
	if err != nil {
		t.Fatalf("persistFallbackSecrets failed: %v", err)
	}

	fallbackPath := strings.TrimSuffix(secretPath, filepath.Ext(secretPath)) + ".secrets"
	data, err := os.ReadFile(fallbackPath)
	if err != nil {
		t.Fatalf("failed to read fallback file: %v", err)
	}

	// Verify the data is encrypted (not plaintext JSON)
	if strings.Contains(string(data), "test-secret") {
		t.Error("fallback file should be encrypted, not contain plaintext secret")
	}

	// Verify we can decrypt and read back
	loaded, err := loadFallbackSecrets(cfg, primarySecret)
	if err != nil {
		t.Fatalf("loadFallbackSecrets failed: %v", err)
	}
	if len(loaded) != 1 || loaded[0].Secret != "test-secret" {
		t.Errorf("expected 1 entry with secret 'test-secret', got %+v", loaded)
	}

	info, err := os.Stat(filepath.Dir(fallbackPath))
	if err != nil {
		t.Fatalf("failed to stat directory: %v", err)
	}
	if runtime.GOOS != "windows" && info.Mode().Perm() != 0700 {
		t.Errorf("expected directory permissions 0700, got %o", info.Mode().Perm())
	}
}

// TestJWTProviderConcurrentVerifyAndRotate exercises VerifyToken concurrently
// with RotateSecret, RevokeToken and AddFallbackSecret — the exact mix an HTTP
// server serving concurrent requests would produce. Run with -race: prior to
// the mutex, this reliably triggered concurrent map read/write and slice
// header races on secret, secrets, secretExpiries and revoked.
func TestJWTProviderConcurrentVerifyAndRotate(t *testing.T) {
	provider := jwtProviderWithSecret(t, "race-test-secret")

	token, err := provider.GenerateToken("race-agent", time.Hour)
	if err != nil {
		t.Fatalf("failed to generate token: %v", err)
	}

	stop := make(chan struct{})
	var readers sync.WaitGroup
	var verifyErrs int32
	for i := 0; i < 8; i++ {
		readers.Add(1)
		go func() {
			defer readers.Done()
			for {
				select {
				case <-stop:
					return
				default:
				}
				if _, err := provider.VerifyToken(token); err != nil {
					atomic.AddInt32(&verifyErrs, 1)
				}
			}
		}()
	}

	var writers sync.WaitGroup
	for i := 0; i < 4; i++ {
		writers.Add(1)
		go func(i int) {
			defer writers.Done()
			for j := 0; j < 25; j++ {
				provider.RotateSecret(fmt.Sprintf("rotated-secret-%d-%d", i, j))
				provider.AddFallbackSecret(fmt.Sprintf("fallback-secret-%d-%d", i, j))
				_ = provider.RevokeToken(fmt.Sprintf("unrelated-jti-%d-%d", i, j))
			}
		}(i)
	}

	writers.Wait()
	close(stop)
	readers.Wait()

	// The original signing secret stays a live fallback for the default 24h
	// grace period, and none of the revoked JTIs match the verified token, so
	// every concurrent verification should have succeeded throughout.
	if n := atomic.LoadInt32(&verifyErrs); n != 0 {
		t.Errorf("expected 0 verification errors during concurrent rotation, got %d", n)
	}

	if _, err := provider.VerifyToken(token); err != nil {
		t.Errorf("token should still verify after concurrent rotation settles: %v", err)
	}
}
