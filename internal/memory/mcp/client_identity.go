package mcp

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"strconv"
	"strings"
)

// fallbackMCPSource is the attribution identity used when no MCP client
// identity can be resolved (no initialize handshake, no clientInfo, and no
// explicit override). It preserves the historical literal actor "mcp".
const fallbackMCPSource = "mcp"

// ClientIdentity identifies the MCP client connected to the stdio session,
// captured from the initialize handshake's clientInfo. InstanceID is a
// stable per-host instance identifier when the client provides one.
type ClientIdentity struct {
	Name       string
	Version    string
	InstanceID string
}

// String renders the identity as "name/version#instance", omitting empty
// parts. It returns "" when nothing is known about the client.
func (c ClientIdentity) String() string {
	if c.Name == "" {
		return ""
	}
	var b strings.Builder
	b.WriteString(c.Name)
	if c.Version != "" {
		b.WriteString("/")
		b.WriteString(c.Version)
	}
	if c.InstanceID != "" {
		b.WriteString("#")
		b.WriteString(c.InstanceID)
	}
	return b.String()
}

// clientInfoParams mirrors the subset of the MCP initialize request params
// needed for attribution. clientId/instanceId/id are not part of the core MCP
// spec but are sent by several clients as a stable per-host instance marker.
type clientInfoParams struct {
	ClientInfo struct {
		Name       string `json:"name"`
		Version    string `json:"version"`
		ClientID   string `json:"clientId"`
		InstanceID string `json:"instanceId"`
		ID         string `json:"id"`
	} `json:"clientInfo"`
}

// parseClientInfo extracts the client identity from an initialize request's
// params. It returns an empty identity when the params are missing, malformed,
// or carry no client name.
func parseClientInfo(params json.RawMessage) ClientIdentity {
	if len(params) == 0 {
		return ClientIdentity{}
	}
	var p clientInfoParams
	if err := json.Unmarshal(params, &p); err != nil {
		return ClientIdentity{}
	}
	ci := p.ClientInfo
	if ci.Name == "" {
		return ClientIdentity{}
	}
	instanceID := ci.InstanceID
	if instanceID == "" {
		instanceID = ci.ClientID
	}
	if instanceID == "" {
		instanceID = ci.ID
	}
	return ClientIdentity{Name: ci.Name, Version: ci.Version, InstanceID: instanceID}
}

// attributionActor resolves the identity used to attribute MCP writes.
// Precedence: explicit override (serve --client-id / SetClientIDOverride) >
// config [mcp] client_id > initialize handshake clientInfo > literal "mcp".
func (s *Server) attributionActor() string {
	s.clientMu.Lock()
	defer s.clientMu.Unlock()
	if s.overrideClientID != "" {
		return s.overrideClientID
	}
	if id := s.clientIdentity.String(); id != "" {
		return id
	}
	return fallbackMCPSource
}

// SetClientIDOverride pins the attribution identity for MCP writes to id,
// winning over both the config value and the initialize handshake. An empty
// id clears the override. This backs the serve --client-id flag.
func (s *Server) SetClientIDOverride(id string) {
	s.clientMu.Lock()
	defer s.clientMu.Unlock()
	s.overrideClientID = id
}

// setHandshakeIdentity records the identity captured from the initialize
// handshake. It is ignored when a handshake identity was already recorded
// (a session only ever has one client).
func (s *Server) setHandshakeIdentity(id ClientIdentity) {
	if id.String() == "" {
		return
	}
	s.clientMu.Lock()
	defer s.clientMu.Unlock()
	if s.clientIdentity.String() == "" {
		s.clientIdentity = id
	}
}

// ServeStdio runs the MCP stdio server on os.Stdin/os.Stdout, capturing the
// initialize handshake's clientInfo for write attribution before delegating
// the session to the underlying MCP server. All logging goes to os.Stderr.
func (s *Server) ServeStdio(ctx context.Context) error {
	return s.serveAttributed(ctx, os.Stdin, os.Stdout)
}

// ServeIO runs the attributed MCP session over the given reader and writer.
// It is the testable entry point equivalent to mcpserver.Server.ServeIO.
func (s *Server) ServeIO(ctx context.Context, r io.Reader, w io.Writer) error {
	return s.serveAttributed(ctx, r, w)
}

// serveAttributed peeks at the first JSON-RPC message of the session — per
// the MCP spec this is the initialize request — captures clientInfo when
// present, then replays the message and delegates the entire stream to the
// underlying server. The capture is a pure observer: the underlying server
// still produces the initialize response and handles every request itself,
// so non-compliant clients (no initialize first) keep working unchanged.
func (s *Server) serveAttributed(ctx context.Context, r io.Reader, w io.Writer) error {
	br := bufio.NewReader(r)
	consumed, req, _, err := readOneRequest(br)
	if err != nil {
		if len(consumed) > 0 {
			// Partial/parse-error message: replay it and let the underlying
			// server surface the protocol error exactly as before.
			return s.MCPServer().ServeIO(ctx, io.MultiReader(bytes.NewReader(consumed), br), w)
		}
		if errors.Is(err, io.EOF) {
			return nil
		}
		return err
	}
	if req != nil && req.Method == "initialize" {
		s.setHandshakeIdentity(parseClientInfo(req.Params))
	}
	return s.MCPServer().ServeIO(ctx, io.MultiReader(bytes.NewReader(consumed), br), w)
}

// ---------------------------------------------------------------------------
// Minimal JSON-RPC framing reader, mirroring the underlying MCP server's
// framing so the peeked first message can be replayed byte-for-byte. It
// supports both Content-Length framed and newline-delimited line messages.
// ---------------------------------------------------------------------------

const maxLineBytes = 1 << 20

type rpcMode int

const (
	rpcModeFramed rpcMode = iota
	rpcModeLine
)

// rawRPCRequest is the JSON-RPC request envelope used for the initialize peek.
type rawRPCRequest struct {
	JSONRPC string          `json:"jsonrpc"`
	ID      any             `json:"id,omitempty"`
	Method  string          `json:"method"`
	Params  json.RawMessage `json:"params,omitempty"`
}

// countingReader records every byte pulled from the underlying reader so the
// consumed message can be replayed into the delegated server.
type countingReader struct {
	r *bufio.Reader
}

func (c *countingReader) Read(p []byte) (int, error) {
	return c.r.Read(p)
}

// readOneRequest reads a single JSON-RPC request from br and returns the
// exact bytes consumed (so the message can be replayed), the parsed request,
// and the wire mode. It mirrors the framing logic of the underlying server.
func readOneRequest(br *bufio.Reader) ([]byte, *rawRPCRequest, rpcMode, error) {
	cr := &countingReader{r: br}
	captured := &bytes.Buffer{}

	line, err := readNonEmptyLine(cr, captured)
	if err != nil {
		return captured.Bytes(), nil, rpcModeFramed, err
	}

	if strings.HasPrefix(line, "{") || !strings.Contains(line, ":") {
		req, perr := parseLineRequest(line)
		return captured.Bytes(), req, rpcModeLine, perr
	}

	var contentLength int
	found := false
	if rest, ok := strings.CutPrefix(line, "Content-Length:"); ok {
		n, err := strconv.Atoi(strings.TrimSpace(rest))
		if err != nil {
			return captured.Bytes(), nil, rpcModeFramed, errors.New("invalid Content-Length: " + strings.TrimSpace(rest))
		}
		contentLength = n
		found = true
	}

	for {
		line, err := readLineLimited(cr, captured)
		if err != nil {
			return captured.Bytes(), nil, rpcModeFramed, err
		}
		line = strings.TrimRight(line, "\r\n")
		if line == "" {
			break
		}
		if rest, ok := strings.CutPrefix(line, "Content-Length:"); ok {
			n, err := strconv.Atoi(strings.TrimSpace(rest))
			if err != nil {
				return captured.Bytes(), nil, rpcModeFramed, errors.New("invalid Content-Length: " + strings.TrimSpace(rest))
			}
			contentLength = n
			found = true
		}
	}
	if !found {
		return captured.Bytes(), nil, rpcModeFramed, errors.New("missing Content-Length header")
	}
	if contentLength <= 0 || contentLength > maxLineBytes {
		return captured.Bytes(), nil, rpcModeFramed, errors.New("invalid Content-Length: " + strconv.Itoa(contentLength))
	}

	body := make([]byte, contentLength)
	if _, err := io.ReadFull(cr, body); err != nil {
		return captured.Bytes(), nil, rpcModeFramed, err
	}
	captured.Write(body)

	var req rawRPCRequest
	if err := json.Unmarshal(body, &req); err != nil {
		return captured.Bytes(), nil, rpcModeFramed, errors.New("parse error: " + err.Error())
	}
	return captured.Bytes(), &req, rpcModeFramed, nil
}

func readNonEmptyLine(cr *countingReader, captured *bytes.Buffer) (string, error) {
	for {
		line, err := readLineLimited(cr, captured)
		if err != nil {
			if err == io.EOF && line != "" {
				return strings.TrimSpace(line), nil
			}
			return "", err
		}
		line = strings.TrimSpace(line)
		if line != "" {
			return line, nil
		}
	}
}

func readLineLimited(cr *countingReader, captured *bytes.Buffer) (string, error) {
	var buf []byte
	for {
		chunk, err := cr.r.ReadSlice('\n')
		captured.Write(chunk)
		if len(buf)+len(chunk) > maxLineBytes {
			return "", errors.New("line exceeds " + strconv.Itoa(maxLineBytes) + " bytes")
		}
		buf = append(buf, chunk...)
		if err == nil {
			return string(buf), nil
		}
		if err == bufio.ErrBufferFull {
			continue
		}
		return string(buf), err
	}
}

func parseLineRequest(line string) (*rawRPCRequest, error) {
	var req rawRPCRequest
	if err := json.Unmarshal([]byte(line), &req); err != nil {
		return nil, errors.New("parse error: " + err.Error())
	}
	return &req, nil
}
