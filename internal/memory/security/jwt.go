package security

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/paths"
	"github.com/danieljustus/symaira-brain/internal/memory/secrets"
)

// DefaultRotationGracePeriod is the default duration that a rotated secret
// remains valid as a fallback after key rotation.
const (
	DefaultRotationGracePeriod = 24 * time.Hour
	jwtIssuer                  = "symaira-memory"
)

// DB interface for revocation persistence, avoiding circular imports.
type RevocationStore interface {
	RevokeToken(jti string) error
	IsTokenRevoked(jti string) (bool, error)
}

// JWTProvider manages API token issuance, validation, revocation, and key rotation.
//
// mu guards secret, secrets, secretExpiries and revoked: VerifyToken runs
// concurrently per HTTP request while RevokeToken/RotateSecret/AddFallbackSecret
// mutate the same state, so unsynchronized access would race.
type JWTProvider struct {
	mu             sync.RWMutex
	secret         []byte
	secrets        [][]byte // fallback keys for rotation grace period
	secretExpiries []time.Time
	revoked        map[string]time.Time
	revStore       RevocationStore
	cfg            *config.Config
	gracePeriod    time.Duration
}

type fallbackEntry struct {
	Secret    string    `json:"secret"`
	ExpiresAt time.Time `json:"expires_at"`
}

type jwtHeader struct {
	Alg string `json:"alg"`
	Typ string `json:"typ"`
}

type JWTPayload struct {
	JWTID     string `json:"jti"`
	Issuer    string `json:"iss"`
	Subject   string `json:"sub"`
	IssuedAt  int64  `json:"iat"`
	ExpiresAt int64  `json:"exp"`
}

// NewJWTProvider configures a JWT provider with a secret and an optional
// persistent revocation store. When store is nil, only in-memory revocation
// is used. The cfg argument supplies the configured secret path; pass nil
// to fall back to the default ~/.config/symmemory/jwt.secret location.
//
// Secret resolution order:
//  1. cfg.JWT.Secret — shared secret reference (symvault://, vault://,
//     env://, or keychain://) resolved with the shared subprocess deadline
//  2. JWT_SECRET_KEY environment variable
//  3. File at cfg.JWT.SecretPath (or default ~/.config/symmemory/jwt.secret)
//  4. Auto-generate and persist a random 32-byte hex secret
func NewJWTProvider(cfg *config.Config, store RevocationStore) (*JWTProvider, error) {
	if cfg == nil {
		cfg = config.Defaults()
	}

	// 1. Try symvault:// resolution from config
	secret, err := secrets.Resolve(cfg.JWT.Secret, "JWT_SECRET_KEY")
	if err != nil {
		// symvault:// resolution failed — propagate error with context
		return nil, fmt.Errorf("JWT secret symvault:// resolution failed: %w", err)
	}

	// 2. Try env var if symvault:// didn't produce a value
	if secret == "" {
		secret = os.Getenv("JWT_SECRET_KEY")
	}

	// 3. Try loading from file
	if secret == "" {
		loaded, loadErr := loadPersistedSecret(cfg)
		if loadErr == nil && loaded != "" {
			secret = loaded
		}
	}

	// 4. Auto-generate as last resort
	if secret == "" {
		generated, genErr := generateAndPersistSecret(cfg)
		if genErr != nil {
			return nil, fmt.Errorf("failed to generate JWT secret: %w", genErr)
		}
		secret = generated
	}

	provider := &JWTProvider{
		secret:      []byte(secret),
		revoked:     make(map[string]time.Time),
		revStore:    store,
		cfg:         cfg,
		gracePeriod: DefaultRotationGracePeriod,
	}

	entries, err := loadFallbackSecrets(cfg, []byte(secret))
	if err != nil {
		fmt.Fprintf(os.Stderr, "warning: failed to load fallback JWT secrets: %v\n", err)
	}
	now := time.Now()
	var validEntries []fallbackEntry
	for _, e := range entries {
		if e.ExpiresAt.After(now) {
			provider.secrets = append(provider.secrets, []byte(e.Secret))
			provider.secretExpiries = append(provider.secretExpiries, e.ExpiresAt)
			validEntries = append(validEntries, e)
		}
	}
	if len(validEntries) != len(entries) {
		if persistErr := persistFallbackSecrets(cfg, validEntries, []byte(secret)); persistErr != nil {
			fmt.Fprintf(os.Stderr, "warning: failed to purge expired fallback secrets: %v\n", persistErr)
		}
	}

	return provider, nil
}

// loadPersistedSecret reads the signing key from the configured path
// or the default ~/.config/symmemory/jwt.secret.
func loadPersistedSecret(cfg *config.Config) (string, error) {
	if cfg == nil {
		cfg = config.Defaults()
	}
	secretPath := cfg.JWT.SecretPath
	if secretPath == "" {
		var err error
		secretPath, err = paths.SecretPath("jwt.secret")
		if err != nil {
			return "", err
		}
	}

	data, err := os.ReadFile(secretPath)
	if err != nil {
		return "", err
	}
	return strings.TrimSpace(string(data)), nil
}

// generateAndPersistSecret creates a random 32-byte hex secret and persists it.
func generateAndPersistSecret(cfg *config.Config) (string, error) {
	bytes := make([]byte, 32)
	if _, err := rand.Read(bytes); err != nil {
		return "", err
	}
	secret := hex.EncodeToString(bytes)

	secretPath := cfg.JWT.SecretPath
	if secretPath == "" {
		var err error
		secretPath, err = paths.SecretPath("jwt.secret")
		if err != nil {
			return "", err
		}
	}

	dir := filepath.Dir(secretPath)
	if err := os.MkdirAll(dir, 0700); err != nil {
		return "", err
	}
	if err := os.WriteFile(secretPath, []byte(secret+"\n"), 0600); err != nil {
		return "", err
	}
	return secret, nil
}

func fallbackSecretsPath(cfg *config.Config) string {
	if cfg == nil {
		cfg = config.Defaults()
	}
	secretPath := cfg.JWT.SecretPath
	if secretPath == "" {
		var err error
		secretPath, err = paths.SecretPath("jwt.secret")
		if err != nil {
			return ""
		}
	}
	return strings.TrimSuffix(secretPath, filepath.Ext(secretPath)) + ".secrets"
}

// deriveFallbackKey derives an AES-256 key from the primary JWT secret and a salt.
// The primary secret already provides high entropy, so we use SHA-256 with domain
// separation via the salt rather than expensive PBKDF2 iterations.
func deriveFallbackKey(primarySecret []byte, salt []byte) []byte {
	h := sha256.New()
	h.Write(primarySecret)
	h.Write(salt)
	return h.Sum(nil)
}

// Output format: [salt(16)][nonce(12)][ciphertext+tag(16)].
func encryptFallbackSecrets(entries []fallbackEntry, primarySecret []byte) ([]byte, error) {
	plaintext, err := json.Marshal(entries)
	if err != nil {
		return nil, fmt.Errorf("marshal fallback entries: %w", err)
	}

	salt := make([]byte, 16)
	if _, err := rand.Read(salt); err != nil {
		return nil, fmt.Errorf("generate salt: %w", err)
	}

	key := deriveFallbackKey(primarySecret, salt)
	block, err := aes.NewCipher(key)
	if err != nil {
		return nil, fmt.Errorf("new cipher: %w", err)
	}

	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return nil, fmt.Errorf("new GCM: %w", err)
	}

	nonce := make([]byte, gcm.NonceSize())
	if _, err := rand.Read(nonce); err != nil {
		return nil, fmt.Errorf("generate nonce: %w", err)
	}

	ciphertext := gcm.Seal(nil, nonce, plaintext, nil)

	payload := make([]byte, len(salt)+len(nonce)+len(ciphertext))
	copy(payload, salt)
	copy(payload[len(salt):], nonce)
	copy(payload[len(salt)+len(nonce):], ciphertext)

	return payload, nil
}

func decryptFallbackSecrets(payload, primarySecret []byte) ([]fallbackEntry, error) {
	if len(payload) < 16+12+16 {
		return nil, errors.New("encrypted payload too short")
	}

	salt := payload[:16]
	nonce := payload[16:28]
	ciphertext := payload[28:]

	key := deriveFallbackKey(primarySecret, salt)
	block, err := aes.NewCipher(key)
	if err != nil {
		return nil, fmt.Errorf("new cipher: %w", err)
	}

	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return nil, fmt.Errorf("new GCM: %w", err)
	}

	plaintext, err := gcm.Open(nil, nonce, ciphertext, nil)
	if err != nil {
		return nil, errors.New("fallback secrets decryption failed: invalid primary secret or corrupted data")
	}

	var entries []fallbackEntry
	if err := json.Unmarshal(plaintext, &entries); err != nil {
		return nil, fmt.Errorf("unmarshal fallback entries: %w", err)
	}
	return entries, nil
}

// loadFallbackSecrets reads fallback secrets and transparently migrates
// any existing plaintext JSON file to the encrypted format.
func loadFallbackSecrets(cfg *config.Config, primarySecret []byte) ([]fallbackEntry, error) {
	path := fallbackSecretsPath(cfg)
	if path == "" {
		return nil, nil
	}
	data, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, err
	}

	// Attempt decryption first (new encrypted format).
	entries, decryptErr := decryptFallbackSecrets(data, primarySecret)
	if decryptErr == nil {
		return entries, nil
	}

	// Fallback to plaintext JSON (legacy format) for transparent migration.
	var legacyEntries []fallbackEntry
	if err := json.Unmarshal(data, &legacyEntries); err != nil {
		return nil, decryptErr // return the original decryption error
	}

	// Migrate: encrypt and re-write.
	if err := persistFallbackSecrets(cfg, legacyEntries, primarySecret); err != nil {
		fmt.Fprintf(os.Stderr, "warning: failed to migrate plaintext fallback secrets to encrypted: %v\n", err)
	}

	return legacyEntries, nil
}

func persistFallbackSecrets(cfg *config.Config, entries []fallbackEntry, primarySecret []byte) error {
	path := fallbackSecretsPath(cfg)
	if path == "" {
		return fmt.Errorf("cannot determine fallback secrets path")
	}
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0700); err != nil {
		return err
	}
	data, err := encryptFallbackSecrets(entries, primarySecret)
	if err != nil {
		return fmt.Errorf("encrypt fallback secrets: %w", err)
	}
	return os.WriteFile(path, data, 0600)
}

// GenerateToken issues a valid signed JWT token for the specified subject (e.g. "extension" or "gpt").
func (jp *JWTProvider) GenerateToken(subject string, duration time.Duration) (string, error) {
	header := jwtHeader{Alg: "HS256", Typ: "JWT"}
	headerBytes, err := json.Marshal(header)
	if err != nil {
		return "", fmt.Errorf("marshal header: %w", err)
	}
	headerEncoded := base64URL(headerBytes)

	now := time.Now()
	jtiBytes := make([]byte, 16)
	if _, err := rand.Read(jtiBytes); err != nil {
		return "", fmt.Errorf("failed to generate JTI: %w", err)
	}
	jti := hex.EncodeToString(jtiBytes)

	payload := JWTPayload{
		JWTID:     jti,
		Issuer:    jwtIssuer,
		Subject:   subject,
		IssuedAt:  now.Unix(),
		ExpiresAt: now.Add(duration).Unix(),
	}
	payloadBytes, err := json.Marshal(payload)
	if err != nil {
		return "", fmt.Errorf("marshal payload: %w", err)
	}
	payloadEncoded := base64URL(payloadBytes)

	// Sign the header and payload
	unsignedToken := headerEncoded + "." + payloadEncoded
	signature := jp.sign(unsignedToken)

	return unsignedToken + "." + signature, nil
}

// VerifyToken validates the signature and expiration of an incoming JWT.
// During key rotation, tokens signed with any known secret are accepted.
func (jp *JWTProvider) VerifyToken(token string) (*JWTPayload, error) {
	parts := strings.Split(token, ".")
	if len(parts) != 3 {
		return nil, errors.New("invalid token structure")
	}

	unsignedToken := parts[0] + "." + parts[1]

	jp.mu.RLock()
	primarySecret := jp.secret
	fallbackSecrets := jp.secrets
	fallbackExpiries := jp.secretExpiries
	jp.mu.RUnlock()

	// Try the primary secret first, then fallback secrets for rotation grace period.
	validSig := false
	now := time.Now()
	secrets := append([][]byte{primarySecret}, fallbackSecrets...)
	for i, s := range secrets {
		if i > 0 && i-1 < len(fallbackExpiries) {
			if exp := fallbackExpiries[i-1]; !exp.IsZero() && exp.Before(now) {
				continue
			}
		}
		if hmac.Equal([]byte(parts[2]), []byte(jp.signWith(unsignedToken, s))) {
			validSig = true
			break
		}
	}
	if !validSig {
		return nil, errors.New("invalid signature")
	}

	// Decode payload
	payloadBytes, err := base64URLDecode(parts[1])
	if err != nil {
		return nil, err
	}

	var payload JWTPayload
	if err := json.Unmarshal(payloadBytes, &payload); err != nil {
		return nil, err
	}
	if strings.TrimSpace(payload.Subject) == "" {
		return nil, errors.New("token has empty subject")
	}
	if payload.Issuer != jwtIssuer {
		return nil, errors.New("invalid issuer")
	}

	// Check in-memory revocation list
	jp.mu.RLock()
	_, isRevoked := jp.revoked[payload.JWTID]
	jp.mu.RUnlock()
	if isRevoked {
		return nil, errors.New("token has been revoked")
	}

	// Check persistent revocation store
	if jp.revStore != nil {
		if isRevoked, err := jp.revStore.IsTokenRevoked(payload.JWTID); err == nil && isRevoked {
			return nil, errors.New("token has been revoked")
		}
	}

	// Check expiration
	if time.Now().Unix() > payload.ExpiresAt {
		return nil, errors.New("token expired")
	}

	return &payload, nil
}

// RevokeToken invalidates a token by its JWT ID so it can no longer be used.
// The in-memory revocation map is always updated first so the token fails
// verification immediately; when a persistent store is available the
// revocation is also persisted for cross-process consistency. A persistence
// failure is NOT silently swallowed — it is returned to the caller so the
// CLI can exit non-zero and the HTTP API can surface a 500, even though the
// in-memory fallback already took effect.
func (jp *JWTProvider) RevokeToken(jti string) error {
	jp.mu.Lock()
	jp.revoked[jti] = time.Now()
	jp.mu.Unlock()
	if jp.revStore != nil {
		if err := jp.revStore.RevokeToken(jti); err != nil {
			return fmt.Errorf("failed to persist token revocation: %w", err)
		}
	}
	return nil
}

// ExtractJTI returns the jti claim of a JWT token without validating its
// signature. It is used by revoke paths that accept a full token value, so a
// token can be revoked by value even after its signature or expiration would
// no longer verify. Callers that already hold the raw jti can pass it
// directly to RevokeToken instead.
func ExtractJTI(token string) (string, error) {
	parts := strings.Split(token, ".")
	if len(parts) != 3 {
		return "", errors.New("invalid token structure")
	}
	payloadBytes, err := base64URLDecode(parts[1])
	if err != nil {
		return "", fmt.Errorf("invalid token payload: %w", err)
	}
	var payload JWTPayload
	if err := json.Unmarshal(payloadBytes, &payload); err != nil {
		return "", fmt.Errorf("invalid token payload: %w", err)
	}
	if payload.JWTID == "" {
		return "", errors.New("token has no jti claim")
	}
	return payload.JWTID, nil
}

// AddFallbackSecret registers an additional signing key for rotation.
// Tokens signed with the fallback key are accepted during the grace period.
func (jp *JWTProvider) AddFallbackSecret(secret string) {
	jp.mu.Lock()
	jp.secrets = append(jp.secrets, []byte(secret))
	jp.mu.Unlock()
}

// RotateSecret replaces the primary signing key and keeps the current key
// as a fallback so existing tokens remain valid during the transition.
// The old secret is persisted to disk with an expiry based on the configured
// grace period, so tokens remain valid across process restarts.
func (jp *JWTProvider) RotateSecret(newSecret string) {
	now := time.Now()
	expiresAt := now.Add(jp.gracePeriod)

	jp.mu.Lock()
	jp.secrets = append(jp.secrets, jp.secret)
	jp.secretExpiries = append(jp.secretExpiries, expiresAt)
	jp.secret = []byte(newSecret)

	var keptSecrets [][]byte
	var keptExpiries []time.Time
	var keptEntries []fallbackEntry
	for i, s := range jp.secrets {
		exp := time.Time{}
		if i < len(jp.secretExpiries) {
			exp = jp.secretExpiries[i]
		}
		if !exp.IsZero() && exp.Before(now) {
			continue
		}
		keptSecrets = append(keptSecrets, s)
		keptExpiries = append(keptExpiries, exp)
		keptEntries = append(keptEntries, fallbackEntry{
			Secret:    string(s),
			ExpiresAt: exp,
		})
	}
	jp.secrets = keptSecrets
	jp.secretExpiries = keptExpiries
	newPrimary := jp.secret
	jp.mu.Unlock()

	if jp.cfg != nil {
		if err := persistFallbackSecrets(jp.cfg, keptEntries, newPrimary); err != nil {
			fmt.Fprintf(os.Stderr, "warning: failed to persist fallback JWT secrets: %v\n", err)
		}
	}
}

func (jp *JWTProvider) sign(text string) string {
	jp.mu.RLock()
	secret := jp.secret
	jp.mu.RUnlock()
	return jp.signWith(text, secret)
}

func (jp *JWTProvider) signWith(text string, secret []byte) string {
	mac := hmac.New(sha256.New, secret)
	mac.Write([]byte(text))
	return base64URL(mac.Sum(nil))
}

func base64URL(b []byte) string {
	encoded := base64.URLEncoding.EncodeToString(b)
	return strings.TrimRight(encoded, "=")
}

func base64URLDecode(s string) ([]byte, error) {
	// Pad if needed
	if idx := len(s) % 4; idx != 0 {
		s += strings.Repeat("=", 4-idx)
	}
	return base64.URLEncoding.DecodeString(s)
}
