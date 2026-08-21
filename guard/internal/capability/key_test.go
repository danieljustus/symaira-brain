package capability

import (
	"errors"
	"os"
	"path/filepath"
	"testing"
)

func TestGenerateKey(t *testing.T) {
	a, err := GenerateKey()
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	b, err := GenerateKey()
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	if len(a) != KeySize {
		t.Errorf("key length = %d, want %d", len(a), KeySize)
	}
	if string(a) == string(b) {
		t.Error("two generated keys are identical")
	}
}

func TestDeriveKey(t *testing.T) {
	master := testKey(t)
	derived, err := DeriveKey(master)
	if err != nil {
		t.Fatalf("DeriveKey() error = %v", err)
	}
	if len(derived) != KeySize {
		t.Errorf("derived key length = %d, want %d", len(derived), KeySize)
	}
	if string(derived) == string(master) {
		t.Error("derived key must differ from master material")
	}
	again, err := DeriveKey(master)
	if err != nil {
		t.Fatalf("DeriveKey() second call error = %v", err)
	}
	if string(derived) != string(again) {
		t.Error("DeriveKey is not deterministic for the same master")
	}
	other, err := DeriveKey(testKey(t))
	if err != nil {
		t.Fatalf("DeriveKey() other-master error = %v", err)
	}
	if string(derived) == string(other) {
		t.Error("two masters must derive different keys")
	}
}

func TestDeriveKey_FailClosed(t *testing.T) {
	for _, master := range [][]byte{nil, {}, []byte("short")} {
		if _, err := DeriveKey(master); !errors.Is(err, ErrNoKeyMaterial) {
			t.Errorf("DeriveKey(%v) error = %v, want %v", master, err, ErrNoKeyMaterial)
		}
	}
}

func TestDefaultKeyPath(t *testing.T) {
	t.Setenv("XDG_DATA_HOME", "/xdg/data")
	wantXDG := filepath.Join("/xdg/data", "symguard", "capability.key")
	if got := DefaultKeyPath(); got != wantXDG {
		t.Errorf("with XDG_DATA_HOME: got %q, want %q", got, wantXDG)
	}
	t.Setenv("XDG_DATA_HOME", "")
	home, err := os.UserHomeDir()
	if err != nil {
		t.Fatalf("UserHomeDir() error = %v", err)
	}
	wantHome := filepath.Join(home, ".local", "share", "symguard", "capability.key")
	if got := DefaultKeyPath(); got != wantHome {
		t.Errorf("without XDG_DATA_HOME: got %q, want %q", got, wantHome)
	}
}

func TestLoadOrCreateKey(t *testing.T) {
	path := filepath.Join(t.TempDir(), "symguard", "capability.key")

	first, err := LoadOrCreateKey(path)
	if err != nil {
		t.Fatalf("LoadOrCreateKey() first error = %v", err)
	}
	if len(first) != KeySize {
		t.Errorf("first key length = %d, want %d", len(first), KeySize)
	}
	info, err := os.Stat(path)
	if err != nil {
		t.Fatalf("Stat() error = %v", err)
	}
	if perm := info.Mode().Perm(); perm != 0o600 {
		t.Errorf("key file mode = %o, want 600", perm)
	}

	second, err := LoadOrCreateKey(path)
	if err != nil {
		t.Fatalf("LoadOrCreateKey() second error = %v", err)
	}
	if string(first) != string(second) {
		t.Error("second load returned different key material")
	}
}

func TestLoadOrCreateKey_FailClosed(t *testing.T) {
	dir := t.TempDir()
	// Corrupt existing key file must error, never regenerate.
	corrupt := filepath.Join(dir, "corrupt.key")
	if err := os.WriteFile(corrupt, []byte("short"), 0o600); err != nil {
		t.Fatalf("WriteFile() error = %v", err)
	}
	if _, err := LoadOrCreateKey(corrupt); !errors.Is(err, ErrNoKeyMaterial) {
		t.Errorf("corrupt key error = %v, want %v", err, ErrNoKeyMaterial)
	}
	// Parent path is a regular file: creation must fail closed.
	blocker := filepath.Join(dir, "blocker")
	if err := os.WriteFile(blocker, []byte("x"), 0o600); err != nil {
		t.Fatalf("WriteFile() error = %v", err)
	}
	if _, err := LoadOrCreateKey(filepath.Join(blocker, "key")); err == nil {
		t.Error("expected error for unwritable key path")
	}
}

func TestLoadKey(t *testing.T) {
	path := filepath.Join(t.TempDir(), "key.bin")
	key := testKey(t)
	if err := os.WriteFile(path, key, 0o600); err != nil {
		t.Fatalf("WriteFile() error = %v", err)
	}
	got, err := LoadKey(path)
	if err != nil {
		t.Fatalf("LoadKey() error = %v", err)
	}
	if string(got) != string(key) {
		t.Error("LoadKey() returned different key material")
	}
}

func TestLoadKey_Missing(t *testing.T) {
	// LoadKey wraps the read error; errors.Is must still see ErrNotExist
	// through the wrap chain (os.IsNotExist alone would not).
	if _, err := LoadKey(filepath.Join(t.TempDir(), "nope.key")); !errors.Is(err, os.ErrNotExist) {
		t.Errorf("LoadKey(missing) error = %v, want os.ErrNotExist", err)
	}
}

func TestLoadKey_TooShort(t *testing.T) {
	// Fail closed: a present-but-too-short key file must not be accepted.
	path := filepath.Join(t.TempDir(), "short.key")
	if err := os.WriteFile(path, []byte("too short"), 0o600); err != nil {
		t.Fatalf("WriteFile() error = %v", err)
	}
	if _, err := LoadKey(path); !errors.Is(err, ErrNoKeyMaterial) {
		t.Errorf("LoadKey(short) error = %v, want %v", err, ErrNoKeyMaterial)
	}
}
