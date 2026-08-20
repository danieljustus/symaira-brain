package security

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
	"crypto/sha256"
	"errors"
	"io"

	"golang.org/x/crypto/pbkdf2"
)

const (
	// PBKDF2Iterations defines the iteration count for key derivation.
	// OWASP 2023 recommends 600,000 for SHA-256.
	PBKDF2Iterations = 600_000

	// FormatVersionV1 identifies the v1 backup format with a 16-byte (128-bit)
	// PBKDF2 salt. Legacy backups used an implicit 8-byte salt with no version prefix.
	FormatVersionV1 byte = 0x01

	saltSizeLegacy = 8
	saltSizeV1     = 16
	nonceSize      = 12
	minCiphertext  = 16
)

// CryptoEngine handles Zero-Knowledge E2E encryption using AES-256-GCM.
type CryptoEngine struct{}

// NewCryptoEngine creates a new crypto instance.
func NewCryptoEngine() *CryptoEngine {
	return &CryptoEngine{}
}

func (ce *CryptoEngine) DeriveKey(passphrase string, salt []byte) []byte {
	return pbkdf2.Key([]byte(passphrase), salt, PBKDF2Iterations, 32, sha256.New)
}

// Encrypt encrypts a raw byte payload using AES-256-GCM with a passphrase.
// Output: [version(1)] [salt(16)] [nonce(12)] [ciphertext].
func (ce *CryptoEngine) Encrypt(plaintext []byte, passphrase string) ([]byte, error) {
	salt := make([]byte, saltSizeV1)
	if _, err := io.ReadFull(rand.Reader, salt); err != nil {
		return nil, err
	}

	key := ce.DeriveKey(passphrase, salt)

	block, err := aes.NewCipher(key)
	if err != nil {
		return nil, err
	}

	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return nil, err
	}

	nonce := make([]byte, gcm.NonceSize())
	if _, err := io.ReadFull(rand.Reader, nonce); err != nil {
		return nil, err
	}

	ciphertext := gcm.Seal(nil, nonce, plaintext, nil)

	payload := make([]byte, 1+len(salt)+len(nonce)+len(ciphertext))
	payload[0] = FormatVersionV1
	copy(payload[1:1+len(salt)], salt)
	copy(payload[1+len(salt):1+len(salt)+len(nonce)], nonce)
	copy(payload[1+len(salt)+len(nonce):], ciphertext)

	return payload, nil
}

// Decrypt decrypts an AES-256-GCM encrypted payload using the passphrase.
// It auto-detects the v1 format (version byte 0x01, 16-byte salt) and falls
// back to the legacy format (no version prefix, 8-byte salt).
func (ce *CryptoEngine) Decrypt(payload []byte, passphrase string) ([]byte, error) {
	if len(payload) == 0 {
		return nil, errors.New("encrypted payload too short")
	}

	// V1: [version=0x01][salt(16)][nonce(12)][ciphertext]
	if payload[0] == FormatVersionV1 && len(payload) >= 1+saltSizeV1+nonceSize+minCiphertext {
		salt := payload[1 : 1+saltSizeV1]
		nonce := payload[1+saltSizeV1 : 1+saltSizeV1+nonceSize]
		ciphertext := payload[1+saltSizeV1+nonceSize:]

		plaintext, err := ce.decryptPayload(passphrase, salt, nonce, ciphertext)
		if err == nil {
			return plaintext, nil
		}
	}

	// Legacy: [salt(8)][nonce(12)][ciphertext]
	if len(payload) < saltSizeLegacy+nonceSize+minCiphertext {
		return nil, errors.New("encrypted payload too short")
	}

	salt := payload[0:saltSizeLegacy]
	nonce := payload[saltSizeLegacy : saltSizeLegacy+nonceSize]
	ciphertext := payload[saltSizeLegacy+nonceSize:]

	return ce.decryptPayload(passphrase, salt, nonce, ciphertext)
}

func (ce *CryptoEngine) decryptPayload(passphrase string, salt, nonce, ciphertext []byte) ([]byte, error) {
	key := ce.DeriveKey(passphrase, salt)

	block, err := aes.NewCipher(key)
	if err != nil {
		return nil, err
	}

	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return nil, err
	}

	plaintext, err := gcm.Open(nil, nonce, ciphertext, nil)
	if err != nil {
		return nil, errors.New("decryption failed: invalid passphrase or corrupted payload")
	}

	return plaintext, nil
}
