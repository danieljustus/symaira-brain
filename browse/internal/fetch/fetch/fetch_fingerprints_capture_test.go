package fetch

// This file is a same-package test overlay, not a production source file: it
// is never compiled into the symbrowse binary. It exists to generate FETCH-002
// compat-wire evidence by driving the real, unmodified newAzureClient /
// azureClient.Fetch code path against a LOOPBACK-only TLS+HTTP/2 server this
// test starts itself.
//
// The only production behavior override is reaching into the unexported
// azureClient.session field (legal same-package access, no source edit) to
// set InsecureSkipVerify — required only because this loopback server uses a
// freshly generated, untrusted self-signed certificate. Everything else
// (profile→browser preset mapping, request construction, SSRF/allowlist
// checks, retry policy, response processing) runs through the unmodified
// production azuretls.go/client.go.
//
// It never contacts a public endpoint or fingerprint service, and it is
// gated behind SYMBROWSE_FETCH_FINGERPRINTS_OUT so a normal `go test ./...`
// run does not execute it.

import (
	"bytes"
	"context"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/md5"
	"crypto/rand"
	"crypto/sha256"
	"crypto/tls"
	"crypto/x509"
	"crypto/x509/pkix"
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"math/big"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"sort"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	"golang.org/x/net/http2/hpack"
)

// ---------------------------------------------------------------------------
// The manifest binds the compiler's actual go-list test closure. It is
// intentionally discovered at capture time; a hand-maintained source subset
// can silently omit a transport-affecting file.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Raw ClientHello capture (unchanged wire-parsing approach, bounds-checked).
// ---------------------------------------------------------------------------

type recordingConn struct {
	net.Conn
	mu  sync.Mutex
	buf bytes.Buffer
}

func (r *recordingConn) Read(p []byte) (int, error) {
	n, err := r.Conn.Read(p)
	if n > 0 {
		r.mu.Lock()
		r.buf.Write(p[:n])
		r.mu.Unlock()
	}
	return n, err
}

func (r *recordingConn) snapshot() []byte {
	r.mu.Lock()
	defer r.mu.Unlock()
	out := make([]byte, r.buf.Len())
	copy(out, r.buf.Bytes())
	return out
}

type clientHelloInfo struct {
	raw             []byte
	legacyVersion   uint16
	cipherSuites    []uint16
	extensions      []uint16
	supportedGroups []uint16
	ecPointFormats  []uint8
}

// parseClientHelloRecord reads the first TLS record off the wire and parses
// it as a ClientHello handshake message. It fails honestly rather than
// guessing when the record is truncated, split across multiple TLS records,
// or not a ClientHello at all.
func parseClientHelloRecord(buf []byte) (*clientHelloInfo, error) {
	if len(buf) < 5 {
		return nil, fmt.Errorf("too short for a TLS record header: %d bytes", len(buf))
	}
	if buf[0] != 0x16 {
		return nil, fmt.Errorf("first record is not a handshake record: content type %d", buf[0])
	}
	recLen := int(buf[3])<<8 | int(buf[4])
	if len(buf) < 5+recLen {
		return nil, fmt.Errorf("truncated TLS record: have %d bytes, record declares %d", len(buf), 5+recLen)
	}
	record := buf[:5+recLen]
	payload := record[5:]
	if len(payload) < 4 {
		return nil, fmt.Errorf("truncated handshake message header")
	}
	if payload[0] != 0x01 {
		return nil, fmt.Errorf("first handshake message is not ClientHello: type %d", payload[0])
	}
	hsLen := int(payload[1])<<16 | int(payload[2])<<8 | int(payload[3])
	if len(payload) < 4+hsLen {
		return nil, fmt.Errorf("ClientHello spans more than one TLS record (unsupported): have %d, want %d", len(payload)-4, hsLen)
	}
	body := payload[4 : 4+hsLen]
	pos := 0
	if len(body) < pos+2 {
		return nil, fmt.Errorf("truncated client_version")
	}
	legacyVersion := uint16(body[pos])<<8 | uint16(body[pos+1])
	pos += 2 + 32
	if len(body) < pos+1 {
		return nil, fmt.Errorf("truncated session_id length")
	}
	sidLen := int(body[pos])
	pos++
	if len(body) < pos+sidLen {
		return nil, fmt.Errorf("truncated session_id")
	}
	pos += sidLen
	if len(body) < pos+2 {
		return nil, fmt.Errorf("truncated cipher_suites length")
	}
	csLen := int(body[pos])<<8 | int(body[pos+1])
	pos += 2
	if csLen%2 != 0 || len(body) < pos+csLen {
		return nil, fmt.Errorf("truncated cipher_suites")
	}
	var ciphers []uint16
	for i := 0; i < csLen; i += 2 {
		ciphers = append(ciphers, uint16(body[pos+i])<<8|uint16(body[pos+i+1]))
	}
	pos += csLen
	if len(body) < pos+1 {
		return nil, fmt.Errorf("truncated compression_methods length")
	}
	cmLen := int(body[pos])
	pos++
	if len(body) < pos+cmLen {
		return nil, fmt.Errorf("truncated compression_methods")
	}
	pos += cmLen

	info := &clientHelloInfo{raw: record, legacyVersion: legacyVersion, cipherSuites: ciphers}
	if pos == len(body) {
		return info, nil
	}
	if len(body) < pos+2 {
		return nil, fmt.Errorf("truncated extensions length")
	}
	extLen := int(body[pos])<<8 | int(body[pos+1])
	pos += 2
	end := pos + extLen
	if end > len(body) {
		return nil, fmt.Errorf("truncated extensions block")
	}
	for pos < end {
		if pos+4 > end {
			return nil, fmt.Errorf("truncated extension header")
		}
		extType := uint16(body[pos])<<8 | uint16(body[pos+1])
		extDataLen := int(body[pos+2])<<8 | int(body[pos+3])
		pos += 4
		if pos+extDataLen > end {
			return nil, fmt.Errorf("truncated extension data for type %d", extType)
		}
		extData := body[pos : pos+extDataLen]
		info.extensions = append(info.extensions, extType)
		switch extType {
		case 10: // supported_groups
			if len(extData) >= 2 {
				glen := int(extData[0])<<8 | int(extData[1])
				gd := extData[2:]
				for i := 0; i+1 < glen && i+1 < len(gd); i += 2 {
					info.supportedGroups = append(info.supportedGroups, uint16(gd[i])<<8|uint16(gd[i+1]))
				}
			}
		case 11: // ec_point_formats
			if len(extData) >= 1 {
				plen := int(extData[0])
				pd := extData[1:]
				for i := 0; i < plen && i < len(pd); i++ {
					info.ecPointFormats = append(info.ecPointFormats, pd[i])
				}
			}
		}
		pos += extDataLen
	}
	return info, nil
}

func joinU16(nums []uint16) string {
	parts := make([]string, len(nums))
	for i, n := range nums {
		parts[i] = strconv.Itoa(int(n))
	}
	return strings.Join(parts, "-")
}

func joinU8(nums []uint8) string {
	parts := make([]string, len(nums))
	for i, n := range nums {
		parts[i] = strconv.Itoa(int(n))
	}
	return strings.Join(parts, "-")
}

func ja3StringFields(version uint16, ciphers, extensions, groups []uint16, points []uint8) string {
	return fmt.Sprintf("%d,%s,%s,%s,%s", version, joinU16(ciphers), joinU16(extensions), joinU16(groups), joinU8(points))
}

func ja3Hash(s string) string {
	sum := md5.Sum([]byte(s)) //nolint:gosec // JA3 is defined as MD5 by the community spec.
	return hex.EncodeToString(sum[:])
}

// greaseValues are the 16 GREASE code points from RFC 8701. Real browsers
// pick one at random per connection for the first cipher suite/extension/
// supported-group entry, so raw JA3 (including GREASE) differs on every
// capture of the *same* profile. Only the GREASE-filtered fingerprint is a
// stable per-profile identity.
var greaseValues = map[uint16]bool{
	0x0A0A: true, 0x1A1A: true, 0x2A2A: true, 0x3A3A: true,
	0x4A4A: true, 0x5A5A: true, 0x6A6A: true, 0x7A7A: true,
	0x8A8A: true, 0x9A9A: true, 0xAAAA: true, 0xBABA: true,
	0xCACA: true, 0xDADA: true, 0xEAEA: true, 0xFAFA: true,
}

func filterGREASE(values []uint16) []uint16 {
	out := make([]uint16, 0, len(values))
	for _, v := range values {
		if !greaseValues[v] {
			out = append(out, v)
		}
	}
	return out
}

// ---------------------------------------------------------------------------
// Loopback capture server: TLS handshake + hand-rolled raw HTTP/2 framing.
//
// Frames are read/written as raw bytes directly against the wire (not via
// golang.org/x/net/http2's buffered Framer) so the exact bytes retained in
// the manifest are guaranteed byte-identical to what was on the wire, with
// no ambiguity from internal buffering. The pinned golang.org/x/net/http2/
// hpack package is still reused for this side's own HPACK decode/encode —
// this does not hand-roll HPACK, only the (trivial, fixed-format) outer
// frame header.
// ---------------------------------------------------------------------------

const http2ClientPreface = "PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n"

const (
	frameTypeData         byte = 0x0
	frameTypeHeaders      byte = 0x1
	frameTypePriority     byte = 0x2
	frameTypeRSTStream    byte = 0x3
	frameTypeSettings     byte = 0x4
	frameTypePing         byte = 0x6
	frameTypeGoAway       byte = 0x7
	frameTypeWindowUpdate byte = 0x8
	frameTypeContinuation byte = 0x9
)

const (
	flagAck        byte = 0x1
	flagEndStream  byte = 0x1
	flagEndHeaders byte = 0x4
	flagPadded     byte = 0x8
	flagPriority   byte = 0x20
)

type rawFrame struct {
	typ      byte
	flags    byte
	streamID uint32
	payload  []byte
}

const maxFramePayload = 1 << 20 // bounded, matches the daemon's own frame limit elsewhere in this repo.

func readRawFrame(r io.Reader) (rawFrame, error) {
	var hdr [9]byte
	if _, err := io.ReadFull(r, hdr[:]); err != nil {
		return rawFrame{}, fmt.Errorf("read frame header: %w", err)
	}
	length := int(hdr[0])<<16 | int(hdr[1])<<8 | int(hdr[2])
	if length > maxFramePayload {
		return rawFrame{}, fmt.Errorf("frame too large: %d bytes", length)
	}
	payload := make([]byte, length)
	if length > 0 {
		if _, err := io.ReadFull(r, payload); err != nil {
			return rawFrame{}, fmt.Errorf("read frame payload: %w", err)
		}
	}
	streamID := binary.BigEndian.Uint32(hdr[5:9]) & 0x7fffffff
	return rawFrame{typ: hdr[3], flags: hdr[4], streamID: streamID, payload: payload}, nil
}

func writeRawFrame(w io.Writer, typ, flags byte, streamID uint32, payload []byte) error {
	var hdr [9]byte
	n := len(payload)
	hdr[0] = byte(n >> 16)
	hdr[1] = byte(n >> 8)
	hdr[2] = byte(n)
	hdr[3] = typ
	hdr[4] = flags
	binary.BigEndian.PutUint32(hdr[5:9], streamID)
	if _, err := w.Write(hdr[:]); err != nil {
		return err
	}
	if len(payload) > 0 {
		_, err := w.Write(payload)
		return err
	}
	return nil
}

func extractHeaderBlock(payload []byte, flags byte) ([]byte, error) {
	pos := 0
	padLen := 0
	if flags&flagPadded != 0 {
		if len(payload) < 1 {
			return nil, fmt.Errorf("padded HEADERS frame too short")
		}
		padLen = int(payload[0])
		pos = 1
	}
	if flags&flagPriority != 0 {
		if len(payload) < pos+5 {
			return nil, fmt.Errorf("HEADERS frame PRIORITY field truncated")
		}
		pos += 5
	}
	end := len(payload) - padLen
	if end < pos || end > len(payload) {
		return nil, fmt.Errorf("HEADERS frame padding invalid")
	}
	return payload[pos:end], nil
}

func parseSettingsPayload(payload []byte) ([]string, error) {
	if len(payload)%6 != 0 {
		return nil, fmt.Errorf("settings payload is not a multiple of 6 bytes: %d", len(payload))
	}
	var out []string
	for i := 0; i+6 <= len(payload); i += 6 {
		id := uint16(payload[i])<<8 | uint16(payload[i+1])
		val := uint32(payload[i+2])<<24 | uint32(payload[i+3])<<16 | uint32(payload[i+4])<<8 | uint32(payload[i+5])
		out = append(out, fmt.Sprintf("%d=%d", id, val))
	}
	return out, nil
}

func encodeOKResponse(body []byte) []byte {
	var buf bytes.Buffer
	enc := hpack.NewEncoder(&buf)
	_ = enc.WriteField(hpack.HeaderField{Name: ":status", Value: "200"})
	_ = enc.WriteField(hpack.HeaderField{Name: "content-length", Value: strconv.Itoa(len(body))})
	return buf.Bytes()
}

const compatLoopbackBody = "FETCH-002 compat loopback response\n"

func generateSelfSignedCert() (tls.Certificate, error) {
	priv, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		return tls.Certificate{}, err
	}
	serial, err := rand.Int(rand.Reader, new(big.Int).Lsh(big.NewInt(1), 128))
	if err != nil {
		return tls.Certificate{}, err
	}
	template := &x509.Certificate{
		SerialNumber: serial,
		Subject:      pkix.Name{CommonName: "fetch002-loopback-capture"},
		NotBefore:    time.Now().Add(-time.Minute),
		NotAfter:     time.Now().Add(time.Hour),
		KeyUsage:     x509.KeyUsageDigitalSignature,
		ExtKeyUsage:  []x509.ExtKeyUsage{x509.ExtKeyUsageServerAuth},
		IPAddresses:  []net.IP{net.ParseIP("127.0.0.1")},
		DNSNames:     []string{"localhost"},
	}
	der, err := x509.CreateCertificate(rand.Reader, template, template, &priv.PublicKey, priv)
	if err != nil {
		return tls.Certificate{}, err
	}
	return tls.Certificate{Certificate: [][]byte{der}, PrivateKey: priv}, nil
}

type captureOutcome struct {
	clientHello        *clientHelloInfo
	alpn               string
	rawSettingsPayload []byte
	rawHeaderBlock     []byte
	headerNames        []string
	h2Settings         []string
	err                error
}

// runCaptureServer terminates exactly one LOOPBACK connection: TLS handshake,
// HTTP/2 preface, the client's SETTINGS frame, and its first HEADERS frame.
// It fails closed (returns an error, does not guess) on CONTINUATION frames,
// GOAWAY, oversized frames, or any parse ambiguity.
func runCaptureServer(ln net.Listener, tlsConfig *tls.Config) captureOutcome {
	return runCaptureServerWithBody(ln, tlsConfig, nil)
}

func runCaptureServerWithBody(ln net.Listener, tlsConfig *tls.Config, responseBody []byte) captureOutcome {
	conn, err := ln.Accept()
	if err != nil {
		return captureOutcome{err: fmt.Errorf("accept: %w", err)}
	}
	defer conn.Close()
	_ = conn.SetDeadline(time.Now().Add(8 * time.Second))

	rec := &recordingConn{Conn: conn}
	tlsConn := tls.Server(rec, tlsConfig)
	handshakeErr := tlsConn.HandshakeContext(context.Background())

	info, parseErr := parseClientHelloRecord(rec.snapshot())
	if parseErr != nil {
		if handshakeErr != nil {
			return captureOutcome{err: fmt.Errorf("handshake failed (%v) and ClientHello unparseable: %w", handshakeErr, parseErr)}
		}
		return captureOutcome{err: fmt.Errorf("parse ClientHello: %w", parseErr)}
	}
	if handshakeErr != nil {
		return captureOutcome{clientHello: info, err: fmt.Errorf("tls handshake failed: %w", handshakeErr)}
	}

	alpn := tlsConn.ConnectionState().NegotiatedProtocol
	if alpn != "h2" {
		return captureOutcome{clientHello: info, alpn: alpn, err: fmt.Errorf("client did not negotiate h2, got %q", alpn)}
	}

	preface := make([]byte, len(http2ClientPreface))
	if _, err := io.ReadFull(tlsConn, preface); err != nil {
		return captureOutcome{clientHello: info, alpn: alpn, err: fmt.Errorf("read h2 preface: %w", err)}
	}
	if string(preface) != http2ClientPreface {
		return captureOutcome{clientHello: info, alpn: alpn, err: fmt.Errorf("bad h2 client preface: %q", preface)}
	}

	if err := writeRawFrame(tlsConn, frameTypeSettings, 0, 0, nil); err != nil {
		return captureOutcome{clientHello: info, alpn: alpn, err: fmt.Errorf("write server settings: %w", err)}
	}

	var rawSettingsPayload []byte
	for {
		frame, err := readRawFrame(tlsConn)
		if err != nil {
			return captureOutcome{clientHello: info, alpn: alpn, err: fmt.Errorf("read h2 frame: %w", err)}
		}
		switch frame.typ {
		case frameTypeSettings:
			if frame.flags&flagAck != 0 {
				continue
			}
			rawSettingsPayload = frame.payload
			if err := writeRawFrame(tlsConn, frameTypeSettings, flagAck, 0, nil); err != nil {
				return captureOutcome{clientHello: info, alpn: alpn, err: fmt.Errorf("settings ack: %w", err)}
			}
		case frameTypeWindowUpdate, frameTypePriority, frameTypeData, frameTypeRSTStream:
			continue
		case frameTypePing:
			if frame.flags&flagAck == 0 {
				if err := writeRawFrame(tlsConn, frameTypePing, flagAck, 0, frame.payload); err != nil {
					return captureOutcome{clientHello: info, alpn: alpn, err: fmt.Errorf("ping ack: %w", err)}
				}
			}
		case frameTypeGoAway:
			return captureOutcome{clientHello: info, alpn: alpn, err: fmt.Errorf("client sent GOAWAY before HEADERS")}
		case frameTypeContinuation:
			return captureOutcome{clientHello: info, alpn: alpn, err: fmt.Errorf("unexpected CONTINUATION frame; failing closed")}
		case frameTypeHeaders:
			if rawSettingsPayload == nil {
				return captureOutcome{clientHello: info, alpn: alpn, err: fmt.Errorf("HEADERS arrived before a client SETTINGS frame")}
			}
			if frame.flags&flagEndHeaders == 0 {
				return captureOutcome{clientHello: info, alpn: alpn, err: fmt.Errorf("HEADERS frame without END_HEADERS (CONTINUATION) is not supported; failing closed")}
			}
			block, err := extractHeaderBlock(frame.payload, frame.flags)
			if err != nil {
				return captureOutcome{clientHello: info, alpn: alpn, err: fmt.Errorf("extract header block: %w", err)}
			}
			var headerNames []string
			dec := hpack.NewDecoder(4096, func(hf hpack.HeaderField) { headerNames = append(headerNames, hf.Name) })
			if _, err := dec.Write(block); err != nil {
				return captureOutcome{clientHello: info, alpn: alpn, err: fmt.Errorf("hpack decode: %w", err)}
			}
			settings, err := parseSettingsPayload(rawSettingsPayload)
			if err != nil {
				return captureOutcome{clientHello: info, alpn: alpn, err: fmt.Errorf("parse settings payload: %w", err)}
			}
			flags := flagEndHeaders
			if len(responseBody) == 0 {
				flags |= flagEndStream
			}
			if err := writeRawFrame(tlsConn, frameTypeHeaders, flags, frame.streamID, encodeOKResponse(responseBody)); err != nil {
				return captureOutcome{clientHello: info, alpn: alpn, err: fmt.Errorf("write response headers: %w", err)}
			}
			if len(responseBody) > 0 {
				if err := writeRawFrame(tlsConn, frameTypeData, flagEndStream, frame.streamID, responseBody); err != nil {
					return captureOutcome{clientHello: info, alpn: alpn, err: fmt.Errorf("write response body: %w", err)}
				}
			}
			// The capture caller closes the one-shot production session after
			// Fetch returns. Send TLS close_notify, then drain that bounded client
			// close before closing the raw socket. Closing TCP with unread TLS/H2
			// bytes produces a Windows RST although the response was complete.
			if err := tlsConn.CloseWrite(); err != nil {
				return captureOutcome{clientHello: info, alpn: alpn, err: fmt.Errorf("close TLS write side: %w", err)}
			}
			_ = tlsConn.SetReadDeadline(time.Now().Add(time.Second))
			_, _ = io.Copy(io.Discard, tlsConn)
			return captureOutcome{
				clientHello:        info,
				alpn:               alpn,
				rawSettingsPayload: rawSettingsPayload,
				rawHeaderBlock:     block,
				headerNames:        headerNames,
				h2Settings:         settings,
			}
		default:
			continue
		}
	}
}

// ---------------------------------------------------------------------------
// Manifest + generator entrypoint
// ---------------------------------------------------------------------------

type buildInputRecord struct {
	Path          string `json:"path"`
	SHA256        string `json:"sha256"`
	GitBlobAtHead string `json:"git_blob_at_head,omitempty"`
}

type compilerIdentity struct {
	GoVersion string `json:"go_version"`
	GOOS      string `json:"goos"`
	GOARCH    string `json:"goarch"`
}

type profileCapture struct {
	Profile                 string   `json:"profile"`
	Protocol                string   `json:"protocol"`
	RawClientHelloB64       string   `json:"raw_client_hello_record_b64"`
	RawSettingsFrameB64     string   `json:"raw_settings_frame_b64"`
	RawHeaderBlockB64       string   `json:"raw_header_block_b64"`
	TLSClientVersion        int      `json:"tls_client_version"`
	CipherSuites            []int    `json:"cipher_suites"`
	Extensions              []int    `json:"extensions"`
	SupportedGroups         []int    `json:"supported_groups"`
	ECPointFormats          []int    `json:"ec_point_formats"`
	JA3String               string   `json:"ja3_string"`
	JA3                     string   `json:"ja3"`
	CipherSuitesNoGrease    []int    `json:"cipher_suites_no_grease"`
	ExtensionsNoGrease      []int    `json:"extensions_no_grease"`
	SupportedGroupsNoGrease []int    `json:"supported_groups_no_grease"`
	JA3StringNoGrease       string   `json:"ja3_string_no_grease"`
	JA3NoGrease             string   `json:"ja3_no_grease"`
	H2Settings              []string `json:"h2_settings"`
	HeaderNames             []string `json:"header_names"`
}

type captureManifest struct {
	SchemaVersion    int                `json:"schema_version"`
	CaptureKind      string             `json:"capture_kind"`
	Note             string             `json:"note"`
	WorktreeHead     string             `json:"worktree_head"`
	Compiler         compilerIdentity   `json:"compiler"`
	ExecutableSHA256 string             `json:"executable_sha256"`
	ExecutablePath   string             `json:"executable_path"`
	BuildInputs      []buildInputRecord `json:"build_inputs"`
	CapturedAtUTC    string             `json:"captured_at_utc"`
	Profiles         []profileCapture   `json:"profiles"`
}

type goListPackage struct {
	Dir          string
	GoFiles      []string
	CgoFiles     []string
	SFiles       []string
	SysoFiles    []string
	CFiles       []string
	HFiles       []string
	CXXFiles     []string
	MFiles       []string
	FFiles       []string
	EmbedFiles   []string
	TestGoFiles  []string
	XTestGoFiles []string
}

func compilerInputClosure(repoRoot string) ([]buildInputRecord, error) {
	browseRoot := filepath.Join(repoRoot, "browse")
	cmd := exec.Command("go", "list", "-deps", "-test", "-json", "./internal/fetch/fetch")
	cmd.Dir = browseRoot
	out, err := cmd.Output()
	if err != nil {
		return nil, fmt.Errorf("go list compiler input closure: %w", err)
	}
	files := map[string]bool{}
	dec := json.NewDecoder(bytes.NewReader(out))
	for {
		var pkg goListPackage
		err := dec.Decode(&pkg)
		if err == io.EOF {
			break
		}
		if err != nil {
			return nil, fmt.Errorf("decode go list compiler input closure: %w", err)
		}
		if pkg.Dir == "" {
			return nil, fmt.Errorf("go list returned a package without Dir")
		}
		for _, names := range [][]string{
			pkg.GoFiles, pkg.CgoFiles, pkg.SFiles, pkg.SysoFiles, pkg.CFiles, pkg.HFiles,
			pkg.CXXFiles, pkg.MFiles, pkg.FFiles, pkg.EmbedFiles, pkg.TestGoFiles, pkg.XTestGoFiles,
		} {
			for _, name := range names {
				target := name
				if !filepath.IsAbs(target) {
					target = filepath.Join(pkg.Dir, target)
				}
				files[filepath.Clean(target)] = true
			}
		}
	}
	files[filepath.Join(browseRoot, "go.mod")] = true
	files[filepath.Join(browseRoot, "go.sum")] = true
	if len(files) == 0 {
		return nil, fmt.Errorf("go list returned an empty compiler input closure")
	}

	blobCmd := exec.Command("git", "-C", repoRoot, "ls-tree", "-r", "--full-tree", "HEAD")
	blobs, err := blobCmd.Output()
	if err != nil {
		return nil, fmt.Errorf("git ls-tree compiler input closure: %w", err)
	}
	gitBlobs := map[string]string{}
	for _, line := range strings.Split(strings.TrimSpace(string(blobs)), "\n") {
		fields := strings.SplitN(line, "\t", 2)
		if len(fields) == 2 {
			meta := strings.Fields(fields[0])
			if len(meta) == 3 {
				gitBlobs[fields[1]] = meta[2]
			}
		}
	}

	paths := make([]string, 0, len(files))
	for path := range files {
		paths = append(paths, path)
	}
	sort.Strings(paths)
	inputs := make([]buildInputRecord, 0, len(paths))
	for _, path := range paths {
		data, err := os.ReadFile(path)
		if err != nil {
			return nil, fmt.Errorf("read compiler input %s: %w", path, err)
		}
		recordPath := filepath.ToSlash(path)
		if rel, relErr := filepath.Rel(repoRoot, path); relErr == nil && !strings.HasPrefix(rel, ".."+string(os.PathSeparator)) && rel != ".." {
			recordPath = filepath.ToSlash(rel)
		}
		record := buildInputRecord{Path: recordPath}
		sum := sha256.Sum256(data)
		record.SHA256 = hex.EncodeToString(sum[:])
		if blob, ok := gitBlobs[recordPath]; ok {
			record.GitBlobAtHead = blob
		}
		inputs = append(inputs, record)
	}
	return inputs, nil
}

func u16sToInts(v []uint16) []int {
	out := make([]int, len(v))
	for i, n := range v {
		out[i] = int(n)
	}
	return out
}

func u8sToInts(v []uint8) []int {
	out := make([]int, len(v))
	for i, n := range v {
		out[i] = int(n)
	}
	return out
}

func captureProfileViaProductionClient(t *testing.T, profile Profile) profileCapture {
	t.Helper()
	result, err := captureProfileWithProductionClient(profile, nil)
	if err != nil {
		t.Fatalf("%s: %v", profile, err)
	}
	return result
}

func captureProfileWithProductionClient(profile Profile, responseBody []byte) (profileCapture, error) {
	return captureProfileWithProductionRequest(profile, responseBody, "GET", nil, nil)
}

func captureProfileWithProductionRequest(profile Profile, responseBody []byte, method string, headers [][2]string, requestBody []byte) (profileCapture, error) {
	cert, err := generateSelfSignedCert()
	if err != nil {
		return profileCapture{}, fmt.Errorf("generate loopback cert: %w", err)
	}
	tlsConfig := &tls.Config{
		Certificates: []tls.Certificate{cert},
		NextProtos:   []string{"h2"},
		MinVersion:   tls.VersionTLS12,
	}
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		return profileCapture{}, fmt.Errorf("listen loopback: %w", err)
	}
	defer ln.Close()
	port := ln.Addr().(*net.TCPAddr).Port

	outcomeCh := make(chan captureOutcome, 1)
	go func() { outcomeCh <- runCaptureServerWithBody(ln, tlsConfig, responseBody) }()

	// newAzureClient is the real, unmodified production constructor.
	c, err := newAzureClient(profile, &clientOptions{timeoutSeconds: 6, maxBodyMB: 1})
	if err != nil {
		return profileCapture{}, fmt.Errorf("newAzureClient: %w", err)
	}
	// Same-package field access, not a source edit: trust only this
	// process's own freshly generated loopback certificate.
	c.session.InsecureSkipVerify = true

	requestHeaders := make(map[string]string, len(headers))
	for _, pair := range headers {
		if len(pair) == 2 {
			requestHeaders[pair[0]] = pair[1]
		}
	}
	_, reqErr := c.Fetch(context.Background(), Request{
		URL:          fmt.Sprintf("https://127.0.0.1:%d/capture", port),
		Method:       method,
		Headers:      requestHeaders,
		Body:         requestBody,
		AllowPrivate: true,
	})
	// Close the capture session before waiting for the test server's bounded
	// close_notify drain. The production client owns the same lifecycle; this
	// makes its ordering explicit in the loopback test harness.
	_ = c.Close()

	var outcome captureOutcome
	select {
	case outcome = <-outcomeCh:
	case <-time.After(10 * time.Second):
		return profileCapture{}, fmt.Errorf("timed out waiting for loopback capture server")
	}
	if outcome.err != nil {
		return profileCapture{}, fmt.Errorf("capture server error: %w", outcome.err)
	}
	if reqErr != nil {
		return profileCapture{}, fmt.Errorf("production client Fetch reported an error against the loopback server: %w", reqErr)
	}

	info := outcome.clientHello
	noGreaseCiphers := filterGREASE(info.cipherSuites)
	noGreaseExt := filterGREASE(info.extensions)
	noGreaseGroups := filterGREASE(info.supportedGroups)

	result := profileCapture{
		Profile:                 string(profile),
		Protocol:                outcome.alpn,
		RawClientHelloB64:       base64.StdEncoding.EncodeToString(info.raw),
		RawSettingsFrameB64:     base64.StdEncoding.EncodeToString(outcome.rawSettingsPayload),
		RawHeaderBlockB64:       base64.StdEncoding.EncodeToString(outcome.rawHeaderBlock),
		TLSClientVersion:        int(info.legacyVersion),
		CipherSuites:            u16sToInts(info.cipherSuites),
		Extensions:              u16sToInts(info.extensions),
		SupportedGroups:         u16sToInts(info.supportedGroups),
		ECPointFormats:          u8sToInts(info.ecPointFormats),
		JA3String:               ja3StringFields(info.legacyVersion, info.cipherSuites, info.extensions, info.supportedGroups, info.ecPointFormats),
		CipherSuitesNoGrease:    u16sToInts(noGreaseCiphers),
		ExtensionsNoGrease:      u16sToInts(noGreaseExt),
		SupportedGroupsNoGrease: u16sToInts(noGreaseGroups),
		JA3StringNoGrease:       ja3StringFields(info.legacyVersion, noGreaseCiphers, noGreaseExt, noGreaseGroups, info.ecPointFormats),
		H2Settings:              outcome.h2Settings,
		HeaderNames:             outcome.headerNames,
	}
	result.JA3 = ja3Hash(result.JA3String)
	result.JA3NoGrease = ja3Hash(result.JA3StringNoGrease)
	return result, nil
}

func makeCaptureManifest(captureKind, note string, profiles []profileCapture) (captureManifest, error) {
	rootOut, err := exec.Command("git", "rev-parse", "--show-toplevel").Output()
	if err != nil {
		return captureManifest{}, fmt.Errorf("git rev-parse --show-toplevel: %w", err)
	}
	repoRoot := strings.TrimSpace(string(rootOut))
	headOut, err := exec.Command("git", "-C", repoRoot, "rev-parse", "HEAD").Output()
	if err != nil {
		return captureManifest{}, fmt.Errorf("git rev-parse HEAD: %w", err)
	}
	inputs, err := compilerInputClosure(repoRoot)
	if err != nil {
		return captureManifest{}, err
	}
	exePath, err := os.Executable()
	if err != nil {
		return captureManifest{}, fmt.Errorf("os.Executable: %w", err)
	}
	exeData, err := os.ReadFile(exePath)
	if err != nil {
		return captureManifest{}, fmt.Errorf("read own binary: %w", err)
	}
	exeSum := sha256.Sum256(exeData)
	return captureManifest{
		SchemaVersion:    2,
		CaptureKind:      captureKind,
		Note:             note,
		WorktreeHead:     strings.TrimSpace(string(headOut)),
		Compiler:         compilerIdentity{GoVersion: runtime.Version(), GOOS: runtime.GOOS, GOARCH: runtime.GOARCH},
		ExecutableSHA256: hex.EncodeToString(exeSum[:]),
		ExecutablePath:   exePath,
		BuildInputs:      inputs,
		CapturedAtUTC:    time.Now().UTC().Format(time.RFC3339),
		Profiles:         profiles,
	}, nil
}

func writeCaptureManifest(path, captureKind, note string, profiles []profileCapture) error {
	manifest, err := makeCaptureManifest(captureKind, note, profiles)
	if err != nil {
		return err
	}
	data, err := json.MarshalIndent(manifest, "", "  ")
	if err != nil {
		return fmt.Errorf("marshal manifest: %w", err)
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return fmt.Errorf("mkdir manifest directory: %w", err)
	}
	if err := os.WriteFile(path, data, 0o600); err != nil {
		return fmt.Errorf("write manifest: %w", err)
	}
	return nil
}

type compatRequestWire struct {
	Type         string      `json:"type"`
	ID           uint64      `json:"id,omitempty"`
	Method       string      `json:"method,omitempty"`
	URL          string      `json:"url,omitempty"`
	Profile      string      `json:"profile,omitempty"`
	Headers      [][2]string `json:"headers,omitempty"`
	Body         string      `json:"body,omitempty"`
	TimeoutMS    uint64      `json:"timeout_ms,omitempty"`
	MaxBodyBytes int         `json:"max_body_bytes,omitempty"`
}

type compatResponseWire struct {
	Type     string       `json:"type"`
	ID       uint64       `json:"id,omitempty"`
	OK       bool         `json:"ok,omitempty"`
	Status   int          `json:"status,omitempty"`
	FinalURL string       `json:"final_url,omitempty"`
	Body     string       `json:"body,omitempty"`
	Error    *compatError `json:"error,omitempty"`
}

type compatError struct {
	Code      string `json:"code"`
	Message   string `json:"message"`
	Retryable bool   `json:"retryable"`
}

func runFetchFingerprintsCompatSidecar(outPath string) int {
	decoder := json.NewDecoder(os.Stdin)
	encoder := json.NewEncoder(os.Stdout)
	var handshake struct {
		Type     string `json:"type"`
		Protocol uint32 `json:"protocol"`
	}
	if err := decoder.Decode(&handshake); err != nil || handshake.Type != "handshake" || handshake.Protocol != 1 {
		return 2
	}
	if err := encoder.Encode(map[string]any{"type": "handshake_ack", "protocol": 1, "component": "symbrowse-go-fetch002", "oracle": "go-azuretls-v0.8.0"}); err != nil {
		return 1
	}
	var profiles []profileCapture
	for {
		var wire compatRequestWire
		if err := decoder.Decode(&wire); err == io.EOF {
			return 0
		} else if err != nil {
			return 1
		}
		response := compatResponseWire{Type: "response", ID: wire.ID}
		profile := ParseProfile(wire.Profile)
		if wire.Type != "request" || !strings.HasPrefix(wire.URL, "https://127.0.0.1/") || wire.Method != "GET" || wire.Body != "" || !isVerifiedPreset(profile) || string(profile) != wire.Profile {
			response.Error = &compatError{Code: "compat_malformed_request", Message: "FETCH-002 sidecar accepts only six bounded loopback GET requests"}
			if err := encoder.Encode(response); err != nil {
				return 1
			}
			continue
		}
		result, err := captureProfileWithProductionRequest(profile, []byte(compatLoopbackBody), wire.Method, wire.Headers, []byte(wire.Body))
		if err != nil {
			response.Error = &compatError{Code: "compat_fetch_failed", Message: err.Error()}
			if encodeErr := encoder.Encode(response); encodeErr != nil {
				return 1
			}
			continue
		}
		result.Profile = wire.Profile
		profiles = append(profiles, result)
		response.OK = true
		response.Status = 200
		response.FinalURL = wire.URL
		response.Body = compatLoopbackBody
		if err := encoder.Encode(response); err != nil {
			return 1
		}
		if len(profiles) == len(verifiedPresets) {
			if err := writeCaptureManifest(outPath, "loopback_hermetic_compat_sidecar", "Diagnostic-only raw-wire capture made by the real Rust CompatClient and this Go same-package sidecar. The sidecar reaches the unmodified production newAzureClient/azureClient.Fetch path; InsecureSkipVerify is confined to this test process and its freshly generated loopback certificate. This is not parity acceptance.", profiles); err != nil {
				return 1
			}
		}
	}
}

func TestMain(m *testing.M) {
	if outPath := os.Getenv("SYMBROWSE_FETCH_FINGERPRINTS_SIDECAR_OUT"); outPath != "" {
		os.Exit(runFetchFingerprintsCompatSidecar(outPath))
	}
	os.Exit(m.Run())
}

// TestGenerateFetchFingerprintsCapture is a generator, not an assertion
// test: it is skipped unless SYMBROWSE_FETCH_FINGERPRINTS_OUT names an
// output path, so a normal `go test ./...` run never executes it.
func TestGenerateFetchFingerprintsCapture(t *testing.T) {
	outPath := os.Getenv("SYMBROWSE_FETCH_FINGERPRINTS_OUT")
	if outPath == "" {
		t.Skip("set SYMBROWSE_FETCH_FINGERPRINTS_OUT to run the FETCH-002 loopback capture generator")
	}

	var profiles []profileCapture
	for _, profile := range []Profile{ProfileChrome, ProfileFirefox, ProfileOpera, ProfileSafari, ProfileEdge, ProfileIos} {
		profiles = append(profiles, captureProfileViaProductionClient(t, profile))
	}
	if err := writeCaptureManifest(outPath, "loopback_hermetic_production_client", "Hermetic capture that drives the unmodified production newAzureClient/azureClient.Fetch code path (internal/fetch/fetch) against a LOOPBACK-only server this test starts itself. Only c.session.InsecureSkipVerify is set from this same-package test overlay, to trust the generated loopback certificate; no production source file was modified. This is evidence of the pinned Go/AzureTLS compat transport's own wire shape, never evidence of a current installed browser identity. Raw TLS/H2 wire bytes are retained so a validator can independently re-derive settings/header names instead of trusting this process's own parse. Raw ja3/ja3_string include random per-connection GREASE values (RFC 8701) and are NOT a stable per-profile identity; ja3_no_grease/ja3_string_no_grease are.", profiles); err != nil {
		t.Fatalf("write manifest: %v", err)
	}
	t.Logf("wrote 6 profile captures to %s", outPath)
}
