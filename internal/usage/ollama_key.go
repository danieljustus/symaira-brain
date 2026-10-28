package usage

import (
	"bytes"
	"crypto/ed25519"
	"encoding/binary"
	"encoding/pem"

	"golang.org/x/crypto/ssh"
)

// Container parsing is structural validation only; x/crypto and crypto/ed25519
// own key decoding, public-key derivation and signatures.
func parseOllamaKey(data []byte) (ed25519.PrivateKey, []byte, error) {
	fail := func() (ed25519.PrivateKey, []byte, error) { return nil, nil, ollamaKeyError() }
	if len(data) > maxCredentialFileBytes || !bytes.HasPrefix(data, []byte("-----BEGIN OPENSSH "+"PRIVATE KEY-----\n")) {
		return fail()
	}
	block, rest := pem.Decode(data)
	if block == nil || block.Type != "OPENSSH PRIVATE KEY" || len(block.Headers) != 0 || len(bytes.TrimSpace(rest)) != 0 {
		return fail()
	}
	const magic = "openssh-key-v1\x00"
	if !bytes.HasPrefix(block.Bytes, []byte(magic)) {
		return fail()
	}
	r := ollamaWire{data: block.Bytes[len(magic):]}
	if string(r.str()) != "none" || string(r.str()) != "none" || len(r.str()) != 0 || r.u32() != 1 {
		return fail()
	}
	public, private := r.str(), r.str()
	if r.bad || len(r.data) != 0 || len(private)%8 != 0 {
		return fail()
	}
	pr := ollamaWire{data: private}
	a, b := pr.u32(), pr.u32()
	if a != b || string(pr.str()) != "ssh-ed25519" {
		return fail()
	}
	repeated, secret := pr.str(), pr.str()
	_ = pr.str() // comment
	if pr.bad || len(secret) != ed25519.PrivateKeySize || len(repeated) != ed25519.PublicKeySize || len(pr.data) == 0 || len(pr.data) > 8 {
		return fail()
	}
	for i, v := range pr.data {
		if int(v) != i+1 {
			return fail()
		}
	}
	key := ed25519.NewKeyFromSeed(secret[:32])
	pub := ollamaWire{data: public}
	if string(pub.str()) != "ssh-ed25519" || !bytes.Equal(pub.str(), key[32:]) || pub.bad || len(pub.data) != 0 || !bytes.Equal(repeated, key[32:]) || !bytes.Equal(secret, key) {
		return fail()
	}
	decoded, err := ssh.ParseRawPrivateKey(data)
	if err != nil {
		return fail()
	}
	parsed, ok := decoded.(*ed25519.PrivateKey)
	if !ok || !bytes.Equal(*parsed, key) {
		return fail()
	}
	return key, public, nil
}

type ollamaWire struct {
	data []byte
	bad  bool
}

func (r *ollamaWire) u32() uint32 {
	if len(r.data) < 4 {
		r.bad = true
		return 0
	}
	n := binary.BigEndian.Uint32(r.data[:4])
	r.data = r.data[4:]
	return n
}
func (r *ollamaWire) str() []byte {
	n := uint64(r.u32())
	if r.bad || n > uint64(len(r.data)) {
		r.bad = true
		return nil
	}
	value := r.data[:int(n)]
	r.data = r.data[int(n):]
	return value
}
