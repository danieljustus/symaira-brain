package security

import (
	"bytes"
	"crypto/aes"
	"crypto/cipher"
	"testing"
)

func TestE2EEncryptionLoop(t *testing.T) {
	crypto := NewCryptoEngine()
	passphrase := "my_ultra_secure_master_password_2026"
	plaintext := []byte("Strict secret: User preferences and API credentials")

	ciphertext, err := crypto.Encrypt(plaintext, passphrase)
	if err != nil {
		t.Fatalf("encryption failed: %v", err)
	}

	if ciphertext[0] != FormatVersionV1 {
		t.Errorf("expected version byte 0x%02x, got 0x%02x", FormatVersionV1, ciphertext[0])
	}

	minV1 := 1 + saltSizeV1 + nonceSize + minCiphertext
	if len(ciphertext) < minV1 {
		t.Errorf("ciphertext too short: got %d, want >= %d", len(ciphertext), minV1)
	}

	if len(ciphertext) <= len(plaintext) {
		t.Errorf("ciphertext is too short, should be longer due to salt and nonce overhead")
	}

	if bytes.Contains(ciphertext, plaintext) {
		t.Errorf("ciphertext leaked plaintext directly")
	}

	decrypted, err := crypto.Decrypt(ciphertext, passphrase)
	if err != nil {
		t.Fatalf("decryption failed: %v", err)
	}

	if !bytes.Equal(decrypted, plaintext) {
		t.Errorf("decrypted payload mismatch:\n  expected: '%s'\n  got:      '%s'", string(plaintext), string(decrypted))
	}

	_, err = crypto.Decrypt(ciphertext, "wrong_password_attempt")
	if err == nil {
		t.Errorf("decryption should fail with incorrect passphrase")
	}
}

func buildLegacyPayload(t *testing.T, plaintext []byte, passphrase string) []byte {
	t.Helper()

	salt := make([]byte, saltSizeLegacy)
	for i := range salt {
		salt[i] = byte(i + 2)
	}

	key := pbkdf2Key(passphrase, salt)
	block, err := aes.NewCipher(key)
	if err != nil {
		t.Fatalf("aes.NewCipher: %v", err)
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		t.Fatalf("cipher.NewGCM: %v", err)
	}

	nonce := make([]byte, nonceSize)
	for i := range nonce {
		nonce[i] = byte(i + 0x10)
	}

	ciphertext := gcm.Seal(nil, nonce, plaintext, nil)

	payload := make([]byte, saltSizeLegacy+nonceSize+len(ciphertext))
	copy(payload[0:saltSizeLegacy], salt)
	copy(payload[saltSizeLegacy:saltSizeLegacy+nonceSize], nonce)
	copy(payload[saltSizeLegacy+nonceSize:], ciphertext)

	return payload
}

func pbkdf2Key(passphrase string, salt []byte) []byte {
	return NewCryptoEngine().DeriveKey(passphrase, salt)
}

func TestDecryptLegacyFormat(t *testing.T) {
	crypto := NewCryptoEngine()
	passphrase := "test_passphrase_legacy"
	plaintext := []byte("legacy data payload")

	legacyPayload := buildLegacyPayload(t, plaintext, passphrase)

	decrypted, err := crypto.Decrypt(legacyPayload, passphrase)
	if err != nil {
		t.Fatalf("legacy decryption failed: %v", err)
	}

	if !bytes.Equal(decrypted, plaintext) {
		t.Errorf("legacy decrypted mismatch: expected '%s', got '%s'", plaintext, decrypted)
	}
}

func TestDecryptLegacyWrongPassphrase(t *testing.T) {
	crypto := NewCryptoEngine()
	passphrase := "correct_passphrase"
	plaintext := []byte("sensitive legacy content")

	legacyPayload := buildLegacyPayload(t, plaintext, passphrase)

	_, err := crypto.Decrypt(legacyPayload, "wrong_passphrase")
	if err == nil {
		t.Error("decryption should fail with incorrect passphrase on legacy payload")
	}
}

func TestDecryptEmptyPayload(t *testing.T) {
	crypto := NewCryptoEngine()
	_, err := crypto.Decrypt(nil, "passphrase")
	if err == nil {
		t.Error("decryption should fail on nil payload")
	}

	_, err = crypto.Decrypt([]byte{}, "passphrase")
	if err == nil {
		t.Error("decryption should fail on empty payload")
	}
}
