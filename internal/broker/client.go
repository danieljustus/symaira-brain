package broker

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"sync"
	"sync/atomic"
)

// maxLineBytes bounds a single newline-delimited JSON-RPC message read from
// a child. This matches corekit/mcpserver's own framed-message cap so a
// misbehaving child cannot force unbounded buffering.
const maxLineBytes = 1 << 20

// clientVersion is reported as this broker's own version in the
// initialize handshake's clientInfo. It intentionally stays a static string
// rather than importing corekit/versionkit: the broker package is a future
// extraction candidate for corekit/mcpclient (see doc.go) and should not
// gain avoidable dependencies.
const clientVersion = "dev"

// Discover resolves the executable path for a child binary. An explicit
// override (from config, see AGENTS.md "Standalone-First") always wins;
// otherwise it falls back to exec.LookPath. A missing binary is reported as
// a plain error — callers (broker.NewManagedServer, doctor) decide whether
// that is fatal or a gracefully degraded server, per AGENTS.md.
func Discover(binaryName, override string) (string, error) {
	if override != "" {
		if _, err := os.Stat(override); err != nil {
			return "", fmt.Errorf("broker: configured binary_path %q for %q: %w", override, binaryName, err)
		}
		return override, nil
	}
	path, err := exec.LookPath(binaryName)
	if err != nil {
		return "", fmt.Errorf("broker: %q not found on PATH: %w", binaryName, err)
	}
	return path, nil
}

// Options configures Spawn.
type Options struct {
	// Args are extra command-line arguments passed to the child binary.
	Args []string
	// Env, if non-nil, replaces the child's environment entirely (as
	// os/exec.Cmd.Env does). A nil Env inherits symbrain's own
	// environment.
	Env []string
	// Stderr receives the child's stderr stream (diagnostics only, per
	// AGENTS.md "Zero Stdio Pollution"). Defaults to os.Stderr when nil.
	Stderr io.Writer
}

// Client is a single child MCP server's stdio JSON-RPC 2.0 connection: one
// spawned process, its stdin/stdout pipes, and the in-flight request
// bookkeeping needed to support concurrent calls with per-call context
// cancellation and timeouts.
//
// Client is safe for concurrent use: multiple goroutines may call
// Initialize/ListTools/CallTool at once (Close waits for none of them —
// callers should quiesce their own calls before Close if a clean shutdown
// matters, e.g. via internal/broker's higher-level lifecycle management).
type Client struct {
	path  string
	cmd   *exec.Cmd
	stdin io.WriteCloser

	writeMu sync.Mutex // serializes writes to stdin

	mu      sync.Mutex
	nextID  int64
	pending map[int64]chan *rpcResponse

	done     chan struct{} // closed once the read loop exits (EOF or error)
	doneErr  error
	doneOnce sync.Once

	exited    chan struct{} // closed once cmd.Wait() returns
	exitErr   error
	closeOnce sync.Once
}

// Spawn starts path as a child process and prepares it for the MCP stdio
// handshake. It does not perform the handshake itself — call Initialize
// after Spawn returns.
func Spawn(path string, opts Options) (*Client, error) {
	cmd := exec.Command(path, opts.Args...)
	if opts.Env != nil {
		cmd.Env = opts.Env
	}
	stderr := opts.Stderr
	if stderr == nil {
		stderr = os.Stderr
	}
	cmd.Stderr = stderr

	stdin, err := cmd.StdinPipe()
	if err != nil {
		return nil, fmt.Errorf("broker: spawn %s: stdin pipe: %w", path, err)
	}
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return nil, fmt.Errorf("broker: spawn %s: stdout pipe: %w", path, err)
	}

	if err := cmd.Start(); err != nil {
		return nil, fmt.Errorf("broker: spawn %s: %w", path, err)
	}

	c := newClient(stdin, stdout)
	c.path = path
	c.cmd = cmd
	go c.waitLoop()

	return c, nil
}

// newClient wires up the protocol-level machinery (request/response
// correlation, the read loop) around an already-open stdin/stdout pair. It
// is shared by Spawn (which supplies a real child process's pipes) and unit
// tests (which supply an io.Pipe pair to simulate a peer without spawning a
// subprocess, see client_test.go). A Client built this way has no backing
// process: Pid returns 0, Process returns nil, and Exited never closes —
// only Spawn's callers get real process lifecycle tracking.
func newClient(stdin io.WriteCloser, stdout io.Reader) *Client {
	c := &Client{
		stdin:   stdin,
		pending: make(map[int64]chan *rpcResponse),
		done:    make(chan struct{}),
		exited:  make(chan struct{}),
	}
	go c.readLoop(stdout)
	return c
}

// waitLoop reaps the child process and records its exit status. It runs for
// the lifetime of the Client so callers never need to call cmd.Wait
// themselves (which os/exec allows only once).
func (c *Client) waitLoop() {
	err := c.cmd.Wait()
	c.exitErr = err
	close(c.exited)
}

// readLoop reads newline-delimited JSON-RPC responses from the child's
// stdout and routes each to its waiting caller. It exits (closing c.done)
// on EOF or a fatal decode error, at which point every still-pending call
// is failed with ClosedError.
func (c *Client) readLoop(stdout io.Reader) {
	br := bufio.NewReaderSize(stdout, 4096)
	var loopErr error

loop:
	for {
		line, err := readLimitedLine(br)
		if len(line) > 0 {
			var resp rpcResponse
			if jsonErr := json.Unmarshal(line, &resp); jsonErr == nil && resp.ID != nil {
				c.deliver(&resp)
			}
			// Malformed lines and notifications (no ID) are silently
			// skipped: this is a client, not a protocol conformance
			// checker, and children never send us notifications today.
		}
		if err != nil {
			if err != io.EOF {
				loopErr = err
			}
			break loop
		}
	}

	c.doneOnce.Do(func() {
		c.doneErr = loopErr
		close(c.done)
	})

	// Fail every call still waiting for a response.
	c.mu.Lock()
	pending := c.pending
	c.pending = nil
	c.mu.Unlock()
	for _, ch := range pending {
		close(ch)
	}
}

func readLimitedLine(br *bufio.Reader) ([]byte, error) {
	line, err := br.ReadSlice('\n')
	if err == bufio.ErrBufferFull {
		// Line longer than the reader's internal buffer: keep reading
		// until '\n' or the hard cap, matching corekit/mcpserver's own
		// bound on a single message.
		buf := append([]byte(nil), line...)
		for err == bufio.ErrBufferFull {
			if len(buf) > maxLineBytes {
				return nil, fmt.Errorf("broker: line exceeds %d bytes", maxLineBytes)
			}
			line, err = br.ReadSlice('\n')
			buf = append(buf, line...)
		}
		line = buf
	}
	line = trimNewline(line)
	return line, err
}

func trimNewline(b []byte) []byte {
	for len(b) > 0 && (b[len(b)-1] == '\n' || b[len(b)-1] == '\r') {
		b = b[:len(b)-1]
	}
	return b
}

func (c *Client) deliver(resp *rpcResponse) {
	c.mu.Lock()
	ch, ok := c.pending[*resp.ID]
	if ok {
		delete(c.pending, *resp.ID)
	}
	c.mu.Unlock()
	if ok {
		ch <- resp
	}
}

// call sends a request and blocks for its response, honoring ctx
// cancellation/deadline and reporting a crashed/closed child distinctly
// from a timeout.
func (c *Client) call(ctx context.Context, method string, params any) (json.RawMessage, error) {
	id := atomic.AddInt64(&c.nextID, 1)

	var raw json.RawMessage
	if params != nil {
		data, err := json.Marshal(params)
		if err != nil {
			return nil, fmt.Errorf("broker: marshal %s params: %w", method, err)
		}
		raw = data
	}

	ch := make(chan *rpcResponse, 1)
	c.mu.Lock()
	if c.pending == nil {
		c.mu.Unlock()
		return nil, &ClosedError{Op: method, Err: c.doneErr}
	}
	c.pending[id] = ch
	c.mu.Unlock()

	req := rpcRequest{JSONRPC: "2.0", ID: &id, Method: method, Params: raw}
	if err := c.write(req); err != nil {
		c.mu.Lock()
		delete(c.pending, id)
		c.mu.Unlock()
		return nil, &ClosedError{Op: method, Err: err}
	}

	select {
	case resp, ok := <-ch:
		if !ok {
			return nil, &ClosedError{Op: method, Err: c.doneErr}
		}
		if resp.Error != nil {
			return nil, &RPCError{Code: resp.Error.Code, Message: resp.Error.Message}
		}
		return resp.Result, nil
	case <-ctx.Done():
		c.mu.Lock()
		delete(c.pending, id)
		c.mu.Unlock()
		if ctx.Err() == context.DeadlineExceeded {
			return nil, &TimeoutError{Op: method}
		}
		return nil, fmt.Errorf("broker: %s canceled: %w", method, ctx.Err())
	case <-c.done:
		return nil, &ClosedError{Op: method, Err: c.doneErr}
	}
}

// notify sends a JSON-RPC notification (no response expected, no id).
func (c *Client) notify(method string, params any) error {
	var raw json.RawMessage
	if params != nil {
		data, err := json.Marshal(params)
		if err != nil {
			return fmt.Errorf("broker: marshal %s params: %w", method, err)
		}
		raw = data
	}
	return c.write(rpcRequest{JSONRPC: "2.0", Method: method, Params: raw})
}

func (c *Client) write(req rpcRequest) error {
	data, err := json.Marshal(req)
	if err != nil {
		return err
	}
	data = append(data, '\n')

	c.writeMu.Lock()
	defer c.writeMu.Unlock()
	_, err = c.stdin.Write(data)
	return err
}

// Initialize performs the MCP initialize handshake: sends "initialize" with
// symbrain's protocol version and clientInfo, then sends the
// "notifications/initialized" notification the spec requires afterward.
func (c *Client) Initialize(ctx context.Context) (*InitializeResult, error) {
	params := initializeParams{
		ProtocolVersion: protocolVersion,
		Capabilities:    map[string]any{},
		ClientInfo:      clientInfo{Name: clientName, Version: clientVersion},
	}
	raw, err := c.call(ctx, "initialize", params)
	if err != nil {
		return nil, err
	}

	var result InitializeResult
	if err := json.Unmarshal(raw, &result); err != nil {
		return nil, fmt.Errorf("broker: parse initialize result: %w", err)
	}

	// Best-effort per the spec: a child that can't take the notification
	// (e.g. it exited right after responding) shouldn't fail an otherwise
	// successful handshake.
	_ = c.notify("notifications/initialized", nil)

	return &result, nil
}

// ListTools calls tools/list and returns the child's advertised tools.
func (c *Client) ListTools(ctx context.Context) ([]Tool, error) {
	raw, err := c.call(ctx, "tools/list", nil)
	if err != nil {
		return nil, err
	}
	var result toolsListResult
	if err := json.Unmarshal(raw, &result); err != nil {
		return nil, fmt.Errorf("broker: parse tools/list result: %w", err)
	}
	return result.Tools, nil
}

// CallTool calls tools/call for name with the given raw JSON arguments
// object (nil for no arguments). A JSON-RPC-level failure (unknown tool,
// invalid params, child-internal error) is returned as *RPCError; a
// context deadline/cancellation as *TimeoutError or a wrapped
// context.Canceled; a crashed/closed child as *ClosedError. A tool-level
// failure (the call reached the tool, which reported its own error) is not
// a Go error at all — it comes back as CallToolResult.IsError == true.
func (c *Client) CallTool(ctx context.Context, name string, arguments json.RawMessage) (*CallToolResult, error) {
	raw, err := c.call(ctx, "tools/call", toolsCallParams{Name: name, Arguments: arguments})
	if err != nil {
		return nil, err
	}
	var result CallToolResult
	if err := json.Unmarshal(raw, &result); err != nil {
		return nil, fmt.Errorf("broker: parse tools/call result: %w", err)
	}
	return &result, nil
}

// Done returns a channel closed once the client's read loop has exited
// (child stdout closed, e.g. because the process crashed or exited). It
// does not by itself mean the process has been reaped — see Exited.
func (c *Client) Done() <-chan struct{} {
	return c.done
}

// Exited returns a channel closed once the child process has been reaped
// (cmd.Wait returned). ExitErr is only meaningful after this channel is
// closed.
func (c *Client) Exited() <-chan struct{} {
	return c.exited
}

// ExitErr returns the error from the child process's exit (as os/exec
// reports it — non-nil for a non-zero exit code or a signal), or nil for a
// clean exit. Only meaningful after Exited() is closed.
func (c *Client) ExitErr() error {
	return c.exitErr
}

// Path returns the child binary's executable path, or "" for a Client with
// no backing process (see newClient).
func (c *Client) Path() string {
	return c.path
}

// Pid returns the child process's PID, or 0 for a Client with no backing
// process (see newClient).
func (c *Client) Pid() int {
	if c.cmd == nil || c.cmd.Process == nil {
		return 0
	}
	return c.cmd.Process.Pid
}

// Close closes the child's stdin (signaling EOF, the cleanest stop for an
// MCP stdio child) and fails every in-flight call. It does not itself
// terminate the process or wait for it to exit — internal/broker's
// lifecycle layer (#13) owns the SIGTERM/kill cascade, since only it knows
// how many children need to shut down together and within what deadline.
func (c *Client) Close() error {
	var err error
	c.closeOnce.Do(func() {
		err = c.stdin.Close()
	})
	return err
}

// Process returns the underlying *os.Process for signal delivery
// (lifecycle management). Returns nil for a Client with no backing process
// (see newClient); always non-nil for a *Client obtained from Spawn, since
// Spawn already turns a failure to start into an error.
func (c *Client) Process() *os.Process {
	if c.cmd == nil {
		return nil
	}
	return c.cmd.Process
}
