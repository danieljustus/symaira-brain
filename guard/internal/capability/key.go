package capability

import (
	"crypto/hkdf"
	"crypto/rand"
	"crypto/sha256"
	"fmt"
	"os"
	"path/filepath"

	"github.com/danieljustus/symaira-guard/internal/config"
)

// KeySize is the length of key material in bytes (256 bits).
const KeySize = 32

// deriveInfo is the HKDF domain-separation label. The signing key is never
// the raw key material: it is HKDF-SHA256 output keyed by the material
// under this fixed label, so the same master key can never leak into
// another purpose domain.
const deriveInfo = "symguard:capability:token-signing:v1"

// GenerateKey returns 32 random bytes of key material (crypto/rand).
func GenerateKey() ([]byte, error) {
	k := make([]byte, KeySize)
	if _, err := rand.Read(k); err != nil {
		return nil, fmt.Errorf("capability: generate key: %w", err)
	}
	return k, nil
}

// DeriveKey derives the token signing key from master key material via
// HKDF-SHA256 under a fixed domain-separation label. It fails closed:
// empty or too-short material returns ErrNoKeyMaterial rather than a weak
// key.
func DeriveKey(master []byte) ([]byte, error) {
	if len(master) < KeySize {
		return nil, fmt.Errorf("%w: key material must be at least %d bytes, got %d",
			ErrNoKeyMaterial, KeySize, len(master))
	}
	key, err := hkdf.Key(sha256.New, master, nil, deriveInfo, KeySize)
	if err != nil {
		return nil, fmt.Errorf("capability: derive key: %w", err)
	}
	return key, nil
}

// DefaultKeyPath returns the XDG data directory path for the capability
// key:
//
//	$XDG_DATA_HOME/symguard/capability.key
//
// with a fallback to ~/.local/share/symguard/capability.key when
// XDG_DATA_HOME is unset (AGENTS.md XDG paths).
func DefaultKeyPath() string {
	return filepath.Join(config.DataDir(), "capability.key")
}

// LoadKey reads key material from path. Missing or unreadable material is
// an error — fail closed, never regenerate silently (regeneration would
// silently invalidate every outstanding token).
func LoadKey(path string) ([]byte, error) {
	raw, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("capability: load key %s: %w", path, err)
	}
	if len(raw) < KeySize {
		return nil, fmt.Errorf("%w: key file %s is %d bytes, want at least %d",
			ErrNoKeyMaterial, path, len(raw), KeySize)
	}
	return raw, nil
}

// LoadOrCreateKey returns the key material at path, generating and
// persisting a fresh 32-byte key (0600 perms) on first run. Existing
// material is returned as-is; a present-but-corrupt key, an unreadable
// key, or a failed write all fail closed with an error — a corrupt file is
// never silently replaced.
func LoadOrCreateKey(path string) ([]byte, error) {
	raw, err := os.ReadFile(path)
	switch {
	case err == nil:
		if len(raw) < KeySize {
			return nil, fmt.Errorf("%w: key file %s is %d bytes, want at least %d",
				ErrNoKeyMaterial, path, len(raw), KeySize)
		}
		return raw, nil
	case !os.IsNotExist(err):
		return nil, fmt.Errorf("capability: load key %s: %w", path, err)
	}
	// First run: generate and persist.
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return nil, fmt.Errorf("capability: create key dir: %w", err)
	}
	key, err := GenerateKey()
	if err != nil {
		return nil, err
	}
	if err := os.WriteFile(path, key, 0o600); err != nil {
		return nil, fmt.Errorf("capability: write key %s: %w", path, err)
	}
	// WriteFile honors 0600 minus umask; re-assert the mode so the key is
	// never group/world readable even under a permissive umask.
	if err := os.Chmod(path, 0o600); err != nil {
		return nil, fmt.Errorf("capability: chmod key %s: %w", path, err)
	}
	return key, nil
}
