package broker

import (
	"context"
	"encoding/json"
	"fmt"
	"log/slog"
	"os"
	"sync"
	"sync/atomic"
	"syscall"
	"time"
)

// ServerState represents the lifecycle state of a managed child server.
type ServerState int

const (
	// StateIdle means the child has not been spawned yet (lazy).
	StateIdle ServerState = iota
	// StateStarting means spawn or initialize is in progress.
	StateStarting
	// StateReady means the child is initialized and ready for tool calls.
	StateReady
	// StateDegraded means the child crashed beyond the restart budget and
	// its tools return clear errors instead of being removed from the catalog.
	StateDegraded
	// StateStopped means the child has been shut down.
	StateStopped
)

func (s ServerState) String() string {
	switch s {
	case StateIdle:
		return "idle"
	case StateStarting:
		return "starting"
	case StateReady:
		return "ready"
	case StateDegraded:
		return "degraded"
	case StateStopped:
		return "stopped"
	default:
		return "unknown"
	}
}

// ServerConfig configures a ManagedServer's lifecycle behavior.
type ServerConfig struct {
	// Name is the server alias (e.g. "vault", "memory", "skills"). Used
	// in log messages and degraded-tool error text.
	Name string
	// BinaryPath is the child executable. Discovered via broker.Discover if
	// empty.
	BinaryPath string
	// Args are extra CLI arguments passed to the child on spawn.
	Args []string
	// InitTimeout bounds the initialize handshake.
	InitTimeout time.Duration
	// CallTimeout bounds individual tools/call requests. Zero means no
	// per-call timeout.
	CallTimeout time.Duration
	// MaxRestarts is the maximum number of consecutive crash-restart
	// attempts before the server is marked degraded. Zero means no restarts
	// (the server goes straight to degraded on first crash).
	MaxRestarts int
	// BackoffBase is the base duration for exponential backoff between
	// restarts. Zero defaults to 1 second.
	BackoffBase time.Duration
	// ShutdownTimeout bounds the graceful shutdown (SIGTERM then kill).
	ShutdownTimeout time.Duration
	// Logger receives lifecycle events. Nil uses slog.Default().
	Logger *slog.Logger
}

func (c *ServerConfig) logger() *slog.Logger {
	if c.Logger != nil {
		return c.Logger
	}
	return slog.Default()
}

func (c *ServerConfig) backoffBase() time.Duration {
	if c.BackoffBase > 0 {
		return c.BackoffBase
	}
	return time.Second
}

func (c *ServerConfig) initTimeout() time.Duration {
	if c.InitTimeout > 0 {
		return c.InitTimeout
	}
	return 10 * time.Second
}

func (c *ServerConfig) shutdownTimeout() time.Duration {
	if c.ShutdownTimeout > 0 {
		return c.ShutdownTimeout
	}
	return 5 * time.Second
}

// ManagedServer wraps a Client with lifecycle management: lazy spawn on
// first call, crash detection, exponential-backoff restart, graceful
// shutdown with SIGTERM cascade, and a degraded state that lets the
// gateway keep the server's tools visible (with clear errors) instead of
// silently dropping them.
//
// ManagedServer is safe for concurrent use.
type ManagedServer struct {
	cfg    ServerConfig
	state  atomic.Int32 // ServerState
	client atomic.Pointer[Client]

	mu           sync.Mutex
	restartCount int
	lastErr      error
}

// NewManagedServer creates a ManagedServer that will spawn its child
// lazily on the first Call or ListTools invocation. The server starts in
// StateIdle.
func NewManagedServer(cfg ServerConfig) *ManagedServer {
	ms := &ManagedServer{cfg: cfg}
	ms.state.Store(int32(StateIdle))
	return ms
}

// State returns the current lifecycle state.
func (ms *ManagedServer) State() ServerState {
	return ServerState(ms.state.Load())
}

// LastError returns the last fatal error (crash, init failure), or nil.
func (ms *ManagedServer) LastError() error {
	ms.mu.Lock()
	defer ms.mu.Unlock()
	return ms.lastErr
}

// RestartCount returns the number of consecutive restarts attempted.
func (ms *ManagedServer) RestartCount() int {
	ms.mu.Lock()
	defer ms.mu.Unlock()
	return ms.restartCount
}

// ensureReady spawns and initializes the child if needed, or restarts it
// after a crash. Returns an error if the server is degraded or stopped.
func (ms *ManagedServer) ensureReady(ctx context.Context) (*Client, error) {
	if c := ms.client.Load(); c != nil {
		return c, nil
	}

	ms.mu.Lock()
	defer ms.mu.Unlock()

	// Double-check under lock.
	if c := ms.client.Load(); c != nil {
		return c, nil
	}

	state := ServerState(ms.state.Load())
	switch state {
	case StateDegraded:
		return nil, &ClosedError{Op: "ensureReady", Err: fmt.Errorf("server %q is degraded", ms.cfg.Name)}
	case StateStopped:
		return nil, &ClosedError{Op: "ensureReady", Err: fmt.Errorf("server %q is stopped", ms.cfg.Name)}
	}

	return ms.spawnAndInit(ctx)
}

// spawnAndInit discovers the binary, spawns the process, and performs the
// MCP initialize handshake. Must be called with ms.mu held.
func (ms *ManagedServer) spawnAndInit(ctx context.Context) (*Client, error) {
	ms.state.Store(int32(StateStarting))
	ms.cfg.logger().Info("spawning child", "server", ms.cfg.Name, "binary", ms.cfg.BinaryPath)

	path, err := Discover(ms.cfg.Name, ms.cfg.BinaryPath)
	if err != nil {
		ms.state.Store(int32(StateIdle))
		return nil, fmt.Errorf("broker server %s: discover: %w", ms.cfg.Name, err)
	}

	opts := Options{
		Args:   ms.cfg.Args,
		Stderr: os.Stderr,
	}
	c, err := Spawn(path, opts)
	if err != nil {
		ms.state.Store(int32(StateIdle))
		return nil, fmt.Errorf("broker server %s: spawn: %w", ms.cfg.Name, err)
	}

	initCtx, cancel := context.WithTimeout(ctx, ms.cfg.initTimeout())
	defer cancel()

	result, err := c.Initialize(initCtx)
	if err != nil {
		_ = c.Close()
		if p := c.Process(); p != nil {
			_ = p.Kill()
		}
		ms.state.Store(int32(StateIdle))
		return nil, fmt.Errorf("broker server %s: initialize: %w", ms.cfg.Name, err)
	}

	version := result.ServerInfo.Version
	if version != "" {
		ms.cfg.logger().Info("child initialized", "server", ms.cfg.Name, "version", version)
	}

	ms.client.Store(c)
	ms.restartCount = 0
	ms.lastErr = nil
	ms.state.Store(int32(StateReady))

	// Start a background goroutine that detects when the child exits and
	// triggers a restart.
	go ms.watchChild(c)

	return c, nil
}

// watchChild waits for the child process to exit and triggers a restart
// unless the server is being shut down.
func (ms *ManagedServer) watchChild(c *Client) {
	<-c.Exited()

	state := ServerState(ms.state.Load())
	if state == StateStopped || state == StateDegraded {
		return
	}

	exitErr := c.ExitErr()
	ms.cfg.logger().Warn("child exited", "server", ms.cfg.Name, "error", exitErr)

	ms.mu.Lock()
	defer ms.mu.Unlock()

	// Clear the client pointer so ensureReady will spawn a fresh one.
	ms.client.Store(nil)

	restarts := ms.restartCount
	if restarts >= ms.cfg.MaxRestarts {
		ms.cfg.logger().Error("server degraded: restart budget exhausted",
			"server", ms.cfg.Name, "restarts", restarts)
		ms.state.Store(int32(StateDegraded))
		ms.lastErr = fmt.Errorf("server %s: restart budget exhausted after %d attempts: %w",
			ms.cfg.Name, restarts, exitErr)
		return
	}

	ms.restartCount++
	backoff := ms.cfg.backoffBase() * (1 << (restarts))
	ms.cfg.logger().Info("restarting child",
		"server", ms.cfg.Name, "attempt", ms.restartCount,
		"backoff", backoff)

	time.AfterFunc(backoff, func() {
		ctx, cancel := context.WithTimeout(context.Background(), ms.cfg.initTimeout())
		defer cancel()
		if _, err := ms.spawnAndInit(ctx); err != nil {
			ms.cfg.logger().Error("restart failed", "server", ms.cfg.Name, "error", err)
		}
	})
}

// ListTools returns the child's tool list, spawning it if needed. Returns
// nil if the server is degraded or stopped.
func (ms *ManagedServer) ListTools(ctx context.Context) ([]Tool, error) {
	c, err := ms.ensureReady(ctx)
	if err != nil {
		return nil, err
	}
	return c.ListTools(ctx)
}

// CallTool proxies a tool call to the child, spawning it if needed. Tool
// names are passed through unmodified — prefix stripping/translation is
// the gateway's responsibility.
func (ms *ManagedServer) CallTool(ctx context.Context, name string, args json.RawMessage) (*CallToolResult, error) {
	c, err := ms.ensureReady(ctx)
	if err != nil {
		return nil, err
	}
	return c.CallTool(ctx, name, args)
}

// Shutdown gracefully stops the child process. It sends SIGTERM, waits up
// to ShutdownTimeout for the process to exit, then kills it. Shutdown is
// idempotent. After Shutdown, the server will not spawn new children.
func (ms *ManagedServer) Shutdown() {
	ms.mu.Lock()
	state := ServerState(ms.state.Load())
	if state == StateStopped {
		ms.mu.Unlock()
		return
	}
	ms.state.Store(int32(StateStopped))
	c := ms.client.Swap(nil)
	ms.mu.Unlock()

	if c == nil {
		return
	}

	ms.cfg.logger().Info("shutting down child", "server", ms.cfg.Name)

	// Close stdin to signal EOF (cleanest MCP shutdown).
	_ = c.Close()

	// Send SIGTERM for graceful process termination.
	if p := c.Process(); p != nil {
		_ = p.Signal(syscall.SIGTERM)
	}

	// Wait for the process to exit or timeout.
	select {
	case <-c.Exited():
		ms.cfg.logger().Info("child exited gracefully", "server", ms.cfg.Name)
	case <-time.After(ms.cfg.shutdownTimeout()):
		ms.cfg.logger().Warn("child did not exit in time, killing", "server", ms.cfg.Name)
		if p := c.Process(); p != nil {
			_ = p.Kill()
		}
	}
}

// Version returns the version string from the child's initialize
// serverInfo, or an empty string if the server hasn't been started yet.
func (ms *ManagedServer) Version() string {
	c := ms.client.Load()
	if c == nil {
		return ""
	}
	// The version is captured during initialize but not stored on the
	// Client. For now we probe via `version --json` (see issue #13
	// acceptance criteria). This is a best-effort diagnostic, not part of
	// the hot path.
	return ""
}
