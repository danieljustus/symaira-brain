// Command fetch_fingerprints_gen is a retained DIAGNOSTIC-ONLY probe, not
// the FETCH-002 compatibility gate. It constructs an azuretls-client session
// directly and duplicates internal/fetch/fetch/azuretls.go's unexported
// profile→browser preset switch, so it never actually exercises the
// production newAzureClient/azureClient.Fetch code path — only the same
// underlying transport library in isolation. Its prior output
// (target/fetch002-execution/fetch-fingerprints-capture.json) is preserved
// as genuine direct-AzureTLS diagnostic evidence, not compatibility
// evidence, and this file is kept for that provenance, not extended further.
//
// The actual compatibility-gate generator is the same-package Go test
// overlay internal/fetch/fetch/fetch_fingerprints_capture_test.go
// (TestGenerateFetchFingerprintsCapture), which drives the real, unmodified
// newAzureClient/azureClient.Fetch path and only reaches into the
// unexported azureClient.session field (legal same-package access, not a
// source edit) to trust its own generated loopback certificate. Use that
// overlay (via port/harness/run_fetch_fingerprints_capture.sh or
// `python3 port/harness/run.py --suite fetch-fingerprints`) for any new
// FETCH-002 evidence.
//
// This command still performs a real hermetic TLS ClientHello handshake and
// HTTP/2 request against a LOOPBACK-only server it starts itself, and
// records exact raw bytes plus parsed wire fields (cipher suites,
// extensions, HTTP/2 SETTINGS, header order). It never contacts a public
// endpoint or fingerprint service, and it is never evidence of a current
// installed Chrome/Firefox/Safari/Edge/Opera identity.
package main

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
	"flag"
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
	"time"

	azuretls "github.com/Noooste/azuretls-client"
	"golang.org/x/net/http2/hpack"
)

// sixProfiles must match internal/fetch/fetch's verifiedPresets exactly.
var sixProfiles = []string{"chrome", "firefox", "opera", "safari", "edge", "ios"}

func azuretlsBrowserForCapture(profile string) string {
	switch profile {
	case "firefox":
		return azuretls.Firefox
	case "opera":
		return azuretls.Opera
	case "safari":
		return azuretls.Safari
	case "edge":
		return azuretls.Edge
	case "ios":
		return azuretls.Ios
	default:
		return azuretls.Chrome
	}
}

// ---------------------------------------------------------------------------
// Raw ClientHello capture
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

type clientHello struct {
	raw             []byte
	legacyVersion   uint16
	cipherSuites    []uint16
	extensions      []uint16
	supportedGroups []uint16
	ecPointFormats  []uint8
}

// parseClientHelloRecord reads the first TLS record off the wire and parses
// it as a ClientHello handshake message. It fails honestly (returns an
// error) rather than guessing when the record is truncated, split across
// multiple TLS records, or not a ClientHello at all.
func parseClientHelloRecord(buf []byte) (*clientHello, error) {
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
	pos += 2
	pos += 32 // random
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

	info := &clientHello{raw: record, legacyVersion: legacyVersion, cipherSuites: ciphers}
	if pos == len(body) {
		return info, nil // no extensions block present
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

func ja3String(info *clientHello) string {
	return fmt.Sprintf("%d,%s,%s,%s,%s",
		info.legacyVersion,
		joinU16(info.cipherSuites),
		joinU16(info.extensions),
		joinU16(info.supportedGroups),
		joinU8(info.ecPointFormats),
	)
}

func ja3Hash(s string) string {
	sum := md5.Sum([]byte(s))
	return hex.EncodeToString(sum[:])
}

// ---------------------------------------------------------------------------
// Loopback capture server: TLS + raw HTTP/2 frames
// ---------------------------------------------------------------------------

type captureOutcome struct {
	clientHello        *clientHello
	h2Settings         []string
	headerNames        []string
	rawSettingsPayload []byte
	rawHeaderBlock     []byte
	alpn               string
	err                error
}

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

const http2ClientPreface = "PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n"

type rawFrame struct {
	typ, flags byte
	streamID   uint32
	payload    []byte
}

func readRawFrame(r io.Reader) (rawFrame, error) {
	var header [9]byte
	if _, err := io.ReadFull(r, header[:]); err != nil {
		return rawFrame{}, fmt.Errorf("read frame header: %w", err)
	}
	length := int(header[0])<<16 | int(header[1])<<8 | int(header[2])
	if length > 1<<20 {
		return rawFrame{}, fmt.Errorf("frame too large: %d bytes", length)
	}
	payload := make([]byte, length)
	if _, err := io.ReadFull(r, payload); err != nil {
		return rawFrame{}, fmt.Errorf("read frame payload: %w", err)
	}
	return rawFrame{typ: header[3], flags: header[4], streamID: binary.BigEndian.Uint32(header[5:]) & 0x7fffffff, payload: payload}, nil
}

func writeRawFrame(w io.Writer, typ, flags byte, streamID uint32, payload []byte) error {
	var header [9]byte
	header[0], header[1], header[2] = byte(len(payload)>>16), byte(len(payload)>>8), byte(len(payload))
	header[3], header[4] = typ, flags
	binary.BigEndian.PutUint32(header[5:], streamID)
	if _, err := w.Write(header[:]); err != nil {
		return err
	}
	_, err := w.Write(payload)
	return err
}

func extractHeaderBlock(payload []byte, flags byte) ([]byte, error) {
	pos, padding := 0, 0
	if flags&0x8 != 0 {
		if len(payload) == 0 {
			return nil, fmt.Errorf("padded HEADERS frame is empty")
		}
		padding, pos = int(payload[0]), 1
	}
	if flags&0x20 != 0 {
		if len(payload) < pos+5 {
			return nil, fmt.Errorf("HEADERS priority field is truncated")
		}
		pos += 5
	}
	end := len(payload) - padding
	if end < pos {
		return nil, fmt.Errorf("HEADERS padding is invalid")
	}
	return payload[pos:end], nil
}

func parseSettingsPayload(payload []byte) ([]string, error) {
	if len(payload)%6 != 0 {
		return nil, fmt.Errorf("settings payload is not a multiple of six bytes")
	}
	settings := make([]string, 0, len(payload)/6)
	for pos := 0; pos < len(payload); pos += 6 {
		settings = append(settings, fmt.Sprintf("%d=%d", binary.BigEndian.Uint16(payload[pos:]), binary.BigEndian.Uint32(payload[pos+2:])))
	}
	return settings, nil
}

func runCaptureServer(ln net.Listener, tlsConfig *tls.Config) captureOutcome {
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

	if err := writeRawFrame(tlsConn, 0x4, 0, 0, nil); err != nil {
		return captureOutcome{clientHello: info, alpn: alpn, err: fmt.Errorf("write server settings: %w", err)}
	}

	var rawSettings []byte
	for {
		frame, err := readRawFrame(tlsConn)
		if err != nil {
			return captureOutcome{clientHello: info, alpn: alpn, err: fmt.Errorf("read h2 frame: %w", err)}
		}
		switch frame.typ {
		case 0x4:
			if frame.flags&0x1 != 0 {
				continue
			}
			rawSettings = frame.payload
			if err := writeRawFrame(tlsConn, 0x4, 0x1, 0, nil); err != nil {
				return captureOutcome{clientHello: info, alpn: alpn, err: fmt.Errorf("settings ack: %w", err)}
			}
		case 0x6:
			if frame.flags&0x1 == 0 {
				if err := writeRawFrame(tlsConn, 0x6, 0x1, 0, frame.payload); err != nil {
					return captureOutcome{clientHello: info, alpn: alpn, err: fmt.Errorf("ping ack: %w", err)}
				}
			}
		case 0x1:
			if rawSettings == nil || frame.flags&0x4 == 0 {
				return captureOutcome{clientHello: info, alpn: alpn, err: fmt.Errorf("unsupported HEADERS framing; failing closed")}
			}
			block, err := extractHeaderBlock(frame.payload, frame.flags)
			if err != nil {
				return captureOutcome{clientHello: info, alpn: alpn, err: err}
			}
			var headerNames []string
			dec := hpack.NewDecoder(4096, func(hf hpack.HeaderField) {
				headerNames = append(headerNames, hf.Name)
			})
			if _, err := dec.Write(block); err != nil {
				return captureOutcome{clientHello: info, alpn: alpn, err: fmt.Errorf("hpack decode: %w", err)}
			}
			var respBuf bytes.Buffer
			henc := hpack.NewEncoder(&respBuf)
			_ = henc.WriteField(hpack.HeaderField{Name: ":status", Value: "200"})
			_ = henc.WriteField(hpack.HeaderField{Name: "content-length", Value: "0"})
			if err := writeRawFrame(tlsConn, 0x1, 0x5, frame.streamID, respBuf.Bytes()); err != nil {
				return captureOutcome{clientHello: info, alpn: alpn, err: fmt.Errorf("write response headers: %w", err)}
			}
			settings, err := parseSettingsPayload(rawSettings)
			if err != nil {
				return captureOutcome{clientHello: info, alpn: alpn, err: err}
			}
			return captureOutcome{clientHello: info, alpn: alpn, h2Settings: settings, headerNames: headerNames, rawSettingsPayload: rawSettings, rawHeaderBlock: block}
		default:
			// PRIORITY, WINDOW_UPDATE etc. — expected bookkeeping, ignore.
		}
	}
}

// ---------------------------------------------------------------------------
// Per-profile orchestration
// ---------------------------------------------------------------------------

type profileCapture struct {
	Profile           string   `json:"profile"`
	Protocol          string   `json:"protocol"`
	RawClientHelloB64 string   `json:"raw_client_hello_record_b64"`
	RawSettingsB64    string   `json:"raw_settings_frame_b64"`
	RawHeaderBlockB64 string   `json:"raw_header_block_b64"`
	TLSClientVersion  int      `json:"tls_client_version"`
	CipherSuites      []int    `json:"cipher_suites"`
	Extensions        []int    `json:"extensions"`
	SupportedGroups   []int    `json:"supported_groups"`
	ECPointFormats    []int    `json:"ec_point_formats"`
	JA3String         string   `json:"ja3_string"`
	JA3               string   `json:"ja3"`
	H2Settings        []string `json:"h2_settings"`
	HeaderNames       []string `json:"header_names"`
	RequestError      string   `json:"request_error,omitempty"`
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

func captureProfile(profile string) (profileCapture, error) {
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
	go func() { outcomeCh <- runCaptureServer(ln, tlsConfig) }()

	sess := azuretls.NewSession()
	sess.Browser = azuretlsBrowserForCapture(profile)
	sess.InsecureSkipVerify = true
	sess.TimeOut = 6 * time.Second
	defer sess.Close()

	_, reqErr := sess.Get(fmt.Sprintf("https://127.0.0.1:%d/capture", port))

	var outcome captureOutcome
	select {
	case outcome = <-outcomeCh:
	case <-time.After(10 * time.Second):
		return profileCapture{}, fmt.Errorf("timed out waiting for loopback capture server")
	}

	result := profileCapture{Profile: profile}
	if reqErr != nil {
		result.RequestError = reqErr.Error()
	}
	if outcome.err != nil {
		if outcome.clientHello == nil {
			return result, fmt.Errorf("profile %s: %w", profile, outcome.err)
		}
		// We got a raw ClientHello even though the rest of the exchange
		// failed; surface both facts honestly rather than discarding the
		// capture or pretending it succeeded.
		result.RequestError = fmt.Sprintf("%s (request error: %s)", outcome.err.Error(), result.RequestError)
	}
	info := outcome.clientHello
	result.Protocol = outcome.alpn
	result.RawClientHelloB64 = base64.StdEncoding.EncodeToString(info.raw)
	result.RawSettingsB64 = base64.StdEncoding.EncodeToString(outcome.rawSettingsPayload)
	result.RawHeaderBlockB64 = base64.StdEncoding.EncodeToString(outcome.rawHeaderBlock)
	result.TLSClientVersion = int(info.legacyVersion)
	result.CipherSuites = u16sToInts(info.cipherSuites)
	result.Extensions = u16sToInts(info.extensions)
	result.SupportedGroups = u16sToInts(info.supportedGroups)
	result.ECPointFormats = u8sToInts(info.ecPointFormats)
	result.JA3String = ja3String(info)
	result.JA3 = ja3Hash(result.JA3String)
	result.H2Settings = outcome.h2Settings
	result.HeaderNames = outcome.headerNames
	if outcome.err != nil {
		return result, fmt.Errorf("profile %s: %w", profile, outcome.err)
	}
	return result, nil
}

// ---------------------------------------------------------------------------
// Manifest / provenance
// ---------------------------------------------------------------------------

type sourceFile struct {
	Path            string `json:"path"`
	SHA256          string `json:"sha256"`
	GitBlobAtHead   string `json:"git_blob_committed_head"`
	MatchesUpstream string `json:"matches_recorded_upstream_blob,omitempty"`
}

type goListPackage struct {
	Dir        string
	GoFiles    []string
	CgoFiles   []string
	SFiles     []string
	SysoFiles  []string
	CFiles     []string
	HFiles     []string
	CXXFiles   []string
	MFiles     []string
	FFiles     []string
	EmbedFiles []string
}

func compilerInputClosure(root string) ([]sourceFile, error) {
	cmd := exec.Command("go", "list", "-deps", "-json", "./scripts/rust-port/cmd/fetchfingerprintsgen")
	cmd.Dir = filepath.Join(root, "browse")
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
		for _, names := range [][]string{pkg.GoFiles, pkg.CgoFiles, pkg.SFiles, pkg.SysoFiles, pkg.CFiles, pkg.HFiles, pkg.CXXFiles, pkg.MFiles, pkg.FFiles, pkg.EmbedFiles} {
			for _, name := range names {
				files[filepath.Clean(filepath.Join(pkg.Dir, name))] = true
			}
		}
	}
	browseRoot := filepath.Join(root, "browse")
	files[filepath.Join(browseRoot, "go.mod")] = true
	files[filepath.Join(browseRoot, "go.sum")] = true
	paths := make([]string, 0, len(files))
	for path := range files {
		paths = append(paths, path)
	}
	sort.Strings(paths)
	blobs := map[string]string{}
	blobOut, err := exec.Command("git", "-C", root, "ls-tree", "-r", "--full-tree", "HEAD").Output()
	if err != nil {
		return nil, fmt.Errorf("git ls-tree compiler input closure: %w", err)
	}
	for _, line := range strings.Split(strings.TrimSpace(string(blobOut)), "\n") {
		fields := strings.SplitN(line, "\t", 2)
		if len(fields) == 2 {
			meta := strings.Fields(fields[0])
			if len(meta) == 3 {
				blobs[fields[1]] = meta[2]
			}
		}
	}
	result := make([]sourceFile, 0, len(paths))
	for _, path := range paths {
		data, err := os.ReadFile(path)
		if err != nil {
			return nil, fmt.Errorf("read compiler input %s: %w", path, err)
		}
		recordPath := filepath.ToSlash(path)
		if rel, relErr := filepath.Rel(root, path); relErr == nil && !strings.HasPrefix(rel, ".."+string(os.PathSeparator)) && rel != ".." {
			recordPath = filepath.ToSlash(rel)
		}
		sum := sha256.Sum256(data)
		record := sourceFile{Path: recordPath, SHA256: hex.EncodeToString(sum[:])}
		record.GitBlobAtHead = blobs[recordPath]
		result = append(result, record)
	}
	return result, nil
}

type manifest struct {
	SchemaVersion     int              `json:"schema_version"`
	CaptureKind       string           `json:"capture_kind"`
	Note              string           `json:"note"`
	WorktreeHead      string           `json:"worktree_head"`
	GoVersion         string           `json:"go_version"`
	ModuleGoDirective string           `json:"module_go_directive"`
	ToolchainNote     string           `json:"toolchain_note"`
	SourceFiles       []sourceFile     `json:"source_files"`
	CapturedAtUTC     string           `json:"captured_at_utc"`
	Profiles          []profileCapture `json:"profiles"`
	Errors            []string         `json:"errors,omitempty"`
}

func repoRoot() string {
	// This file lives at <repoRoot>/browse/scripts/rust-port/, so walk up
	// three directories from the source file's own known layout via cwd,
	// which run.py and go build both invoke from the browse/ module root.
	wd, err := os.Getwd()
	if err != nil {
		return "."
	}
	// If invoked from the browse/ module root, the repo root is one level up.
	if strings.HasSuffix(wd, string(os.PathSeparator)+"browse") {
		return wd[:len(wd)-len("/browse")]
	}
	return wd
}

func main() {
	outPath := flag.String("out", "", "path to write the capture manifest JSON (required)")
	flag.Parse()
	if *outPath == "" {
		fmt.Fprintln(os.Stderr, "error: --out is required")
		os.Exit(2)
	}

	headSHA := strings.TrimSpace(mustGit("rev-parse", "HEAD"))

	sourceFiles, err := compilerInputClosure(repoRoot())
	if err != nil {
		fmt.Fprintln(os.Stderr, "error: compiler input closure:", err)
		os.Exit(1)
	}
	var errs []string

	m := manifest{
		SchemaVersion: 1,
		CaptureKind:   "loopback_hermetic",
		Note: "Hermetic capture against a LOOPBACK-only server this process starts itself. " +
			"This is evidence of the pinned Go/AzureTLS compat transport's own wire shape, " +
			"never evidence of a current installed browser identity. " +
			"raw_client_hello_record_b64 retains the exact bytes read off the wire.",
		WorktreeHead:      headSHA,
		GoVersion:         runtime.Version() + " " + runtime.GOOS + "/" + runtime.GOARCH,
		ModuleGoDirective: "1.26.6",
		ToolchainNote:     "capture was produced with whatever `go` binary invoked this generator; compare go_version above against module_go_directive to see if they match exactly",
		SourceFiles:       sourceFiles,
		CapturedAtUTC:     time.Now().UTC().Format(time.RFC3339),
	}

	for _, profile := range sixProfiles {
		result, err := captureProfile(profile)
		if err != nil {
			errs = append(errs, err.Error())
		}
		m.Profiles = append(m.Profiles, result)
	}
	m.Errors = errs

	data, err := json.MarshalIndent(m, "", "  ")
	if err != nil {
		fmt.Fprintln(os.Stderr, "error: marshal manifest:", err)
		os.Exit(1)
	}
	if err := os.MkdirAll(dirOf(*outPath), 0o755); err != nil {
		fmt.Fprintln(os.Stderr, "error: mkdir output dir:", err)
		os.Exit(1)
	}
	if err := os.WriteFile(*outPath, data, 0o644); err != nil {
		fmt.Fprintln(os.Stderr, "error: write manifest:", err)
		os.Exit(1)
	}
	fmt.Printf("wrote %d profile captures (%d errors) to %s\n", len(m.Profiles), len(errs), *outPath)
	if len(errs) > 0 {
		for _, e := range errs {
			fmt.Fprintln(os.Stderr, "capture error:", e)
		}
		os.Exit(1)
	}
}

func dirOf(path string) string {
	i := strings.LastIndexByte(path, '/')
	if i < 0 {
		return "."
	}
	return path[:i]
}

func mustGit(args ...string) string {
	cmd := exec.Command("git", args...)
	cmd.Dir = repoRoot()
	out, err := cmd.Output()
	if err != nil {
		fmt.Fprintln(os.Stderr, "error: git", strings.Join(args, " "), ":", err)
		os.Exit(1)
	}
	return string(out)
}
