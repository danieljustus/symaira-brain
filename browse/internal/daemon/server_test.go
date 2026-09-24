package daemon

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"net"
	"os"
	"path/filepath"
	"runtime"
	"sync"
	"testing"
	"time"

	"github.com/danieljustus/symaira-browse/internal/session"
)

func startTestServer(t *testing.T, handler Handler) (*Server, string, context.CancelFunc) {
	return startTestServerWithPeerValidator(t, handler, func(net.Conn) error { return nil })
}

func startTestServerWithPeerValidator(t *testing.T, handler Handler, peerValidator func(net.Conn) error) (*Server, string, context.CancelFunc) {
	t.Helper()
	dir, err := os.MkdirTemp("", "sb-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(dir) })
	path := filepath.Join(dir, "default.sock")
	ctx, cancel := context.WithCancel(context.Background())
	server := NewServer(Options{SocketPath: path, Handler: handler, IdleTimeout: -1, OperationTimeout: 25 * time.Millisecond, PeerValidator: peerValidator})
	ready := make(chan error, 1)
	go func() { ready <- server.ListenAndServe(ctx) }()
	deadline := time.Now().Add(time.Second)
	for {
		if info, err := os.Stat(path); err == nil {
			// Listen creates the socket before ListenAndServe applies its
			// restrictive mode. Wait for the security boundary too, rather
			// than racing the chmod in the server goroutine.
			if runtime.GOOS == "windows" || info.Mode().Perm() == 0o600 {
				break
			}
		}
		select {
		case err := <-ready:
			t.Fatalf("test daemon stopped before ready: %v", err)
		default:
		}
		if time.Now().After(deadline) {
			t.Fatal("test daemon did not create its socket")
		}
		time.Sleep(time.Millisecond)
	}
	t.Cleanup(func() { cancel(); _ = server.Close(); <-ready })
	return server, path, cancel
}

func shortSocketTempDir(t *testing.T, prefix string) string {
	t.Helper()
	parent := os.Getenv("SYMAIRA_EXTERNAL_RUNTIME_ROOT")
	if parent == "" {
		// Native macOS socket paths have a small limit; keep local test sockets
		// short when the external harness runtime root is not configured.
		parent = "/tmp"
	}
	dir, err := os.MkdirTemp(parent, prefix)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(dir) })
	return dir
}

func TestDecodeFrameAndStableResponseSchema(t *testing.T) {
	frame, err := DecodeFrame([]byte(`{"cmd":"daemon.status","session":"default","request_id":"r1"}`))
	if err != nil || frame.Cmd != "daemon.status" || frame.RequestID != "r1" {
		t.Fatalf("frame = %#v, err = %v", frame, err)
	}
	if _, err := DecodeFrame([]byte(`{"session":"default"}`)); err == nil {
		t.Fatal("missing command accepted")
	}
	encoded, err := json.Marshal(SuccessResponse(map[string]any{"running": true}, nil))
	if err != nil {
		t.Fatal(err)
	}
	var got map[string]any
	if err := json.Unmarshal(encoded, &got); err != nil {
		t.Fatal(err)
	}
	if got["success"] != true || got["data"] == nil {
		t.Fatalf("response schema = %#v", got)
	}
}

func TestSocketPathValidationAndMode(t *testing.T) {
	if _, err := SocketPathIn(t.TempDir(), "../escape"); err == nil {
		t.Fatal("path traversal session accepted")
	}
	_, path, _ := startTestServer(t, func(context.Context, Frame) (any, []Warning, error) { return map[string]any{"ok": true}, nil, nil })
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	// Windows has no POSIX mode bits (chmod only toggles read-only).
	if runtime.GOOS != "windows" && info.Mode().Perm() != 0o600 {
		t.Fatalf("socket mode = %o", info.Mode().Perm())
	}
	if runtime.GOOS != "windows" {
		directory, err := os.Stat(filepath.Dir(path))
		if err != nil {
			t.Fatal(err)
		}
		if directory.Mode().Perm() != 0o700 {
			t.Fatalf("socket directory mode = %o, want 700", directory.Mode().Perm())
		}
	}
}

func TestDefaultPeerUIDAcceptsCurrentUser(t *testing.T) {
	_, path, _ := startTestServerWithPeerValidator(t, nil, nil)
	client := NewClient(ClientOptions{SocketPath: path, Session: "default", StartDaemon: nil})
	status, err := client.RequestWithoutAutostart(context.Background(), Frame{Cmd: "daemon.status"})
	if err != nil || !status.Success {
		t.Fatalf("same-user status = %#v, err = %v", status, err)
	}
}

func TestOperationTimeoutKeepsConnectionUsable(t *testing.T) {
	_, path, _ := startTestServer(t, func(ctx context.Context, frame Frame) (any, []Warning, error) {
		if frame.Cmd == "slow" {
			<-ctx.Done()
			return nil, nil, ctx.Err()
		}
		return map[string]any{"pong": true}, nil, nil
	})
	conn, err := net.Dial("unix", path)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = conn.Close() }()
	_ = conn.SetDeadline(time.Now().Add(2 * time.Second))
	encoder := json.NewEncoder(conn)
	if err := encoder.Encode(Frame{Cmd: "slow"}); err != nil {
		t.Fatal(err)
	}
	if err := encoder.Encode(Frame{Cmd: "daemon.ping"}); err != nil {
		t.Fatal(err)
	}
	reader := bufio.NewScanner(conn)
	reader.Buffer(make([]byte, 128), maxFrameBytes)
	var first, second Response
	if !reader.Scan() || json.Unmarshal(reader.Bytes(), &first) != nil {
		t.Fatal("missing timeout response")
	}
	if !reader.Scan() || json.Unmarshal(reader.Bytes(), &second) != nil {
		t.Fatal("missing follow-up response")
	}
	if first.Success || first.Error == nil || first.Error.Code != ErrorOperationTimeout {
		t.Fatalf("first response = %#v", first)
	}
	if !second.Success {
		t.Fatalf("connection unusable after timeout: %#v", second)
	}
}

func TestClientDisconnectDoesNotCancelBlockedHandlerOrStopDaemon(t *testing.T) {
	// Keep the socket path short for macOS Unix-domain socket limits.
	dir := shortSocketTempDir(t, "sb-disconnect-")
	path := filepath.Join(dir, "default.sock")
	started := make(chan context.Context, 1)
	release := make(chan struct{})
	finished := make(chan struct{}, 1)
	server := NewServer(Options{
		SocketPath:       path,
		IdleTimeout:      -1,
		OperationTimeout: time.Second,
		PeerValidator:    func(net.Conn) error { return nil },
		Handler: func(ctx context.Context, frame Frame) (any, []Warning, error) {
			if frame.Cmd == "blocked" {
				started <- ctx
				<-release
				finished <- struct{}{}
			}
			return map[string]any{"pong": true}, nil, nil
		},
	})
	serverCtx, cancel := context.WithCancel(context.Background())
	ready := make(chan error, 1)
	go func() { ready <- server.ListenAndServe(serverCtx) }()
	t.Cleanup(func() {
		cancel()
		_ = server.Close()
		<-ready
	})
	deadline := time.Now().Add(time.Second)
	for {
		if _, err := os.Stat(path); err == nil {
			break
		}
		select {
		case err := <-ready:
			t.Fatalf("test daemon stopped before ready: %v", err)
		default:
		}
		if time.Now().After(deadline) {
			t.Fatal("test daemon did not create its socket")
		}
		time.Sleep(time.Millisecond)
	}

	conn, err := net.Dial("unix", path)
	if err != nil {
		t.Fatal(err)
	}
	if err := json.NewEncoder(conn).Encode(Frame{Cmd: "blocked"}); err != nil {
		t.Fatal(err)
	}
	var operationCtx context.Context
	select {
	case operationCtx = <-started:
	case <-time.After(time.Second):
		t.Fatal("blocked handler did not start")
	}
	if err := conn.Close(); err != nil {
		t.Fatal(err)
	}
	select {
	case <-operationCtx.Done():
		t.Fatalf("client disconnect canceled the Go handler: %v", operationCtx.Err())
	case <-time.After(40 * time.Millisecond):
	}
	close(release)
	select {
	case <-finished:
	case <-time.After(time.Second):
		t.Fatal("blocked handler did not finish after release")
	}

	client := NewClient(ClientOptions{SocketPath: path, Session: "default", StartDaemon: nil})
	response, err := client.RequestWithoutAutostart(context.Background(), Frame{Cmd: "daemon.ping"})
	if err != nil || !response.Success {
		t.Fatalf("daemon after disconnected request = %#v, err = %v", response, err)
	}
}

func TestStatusAndStop(t *testing.T) {
	server, path, _ := startTestServer(t, func(context.Context, Frame) (any, []Warning, error) {
		return nil, nil, errors.New("unexpected handler")
	})
	client := NewClient(ClientOptions{SocketPath: path, Session: "default", StartDaemon: nil})
	status, err := client.RequestWithoutAutostart(context.Background(), Frame{Cmd: "daemon.status"})
	if err != nil || !status.Success {
		t.Fatalf("status = %#v, err = %v", status, err)
	}
	data, ok := status.Data.(map[string]any)
	if !ok || data["running"] != true {
		t.Fatalf("status data = %#v", status.Data)
	}
	pid, hasPID := data["pid"].(float64)
	if data["socket"] != path || !hasPID || pid <= 0 {
		t.Fatalf("status identity fields = %#v", data)
	}
	for _, field := range []string{"started_at", "last_activity"} {
		value, ok := data[field].(string)
		if !ok {
			t.Fatalf("status %s = %#v, want timestamp", field, data[field])
		}
		if _, err := time.Parse(time.RFC3339Nano, value); err != nil {
			t.Fatalf("status %s = %q: %v", field, value, err)
		}
	}
	stop, err := client.RequestWithoutAutostart(context.Background(), Frame{Cmd: "daemon.stop"})
	if err != nil || !stop.Success {
		t.Fatalf("stop = %#v, err = %v", stop, err)
	}
	deadline := time.Now().Add(time.Second)
	for time.Now().Before(deadline) {
		if _, err := os.Stat(server.SocketPath()); errors.Is(err, os.ErrNotExist) {
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatalf("daemon socket survived stop: %s", path)
}

func TestIdleTimeoutStopsServer(t *testing.T) {
	dir := shortSocketTempDir(t, "sb-idle-")
	path := filepath.Join(dir, "idle.sock")
	server := NewServer(Options{
		SocketPath:       path,
		Session:          "idle",
		IdleTimeout:      25 * time.Millisecond,
		OperationTimeout: time.Second,
		PeerValidator:    func(net.Conn) error { return nil },
	})
	finished := make(chan error, 1)
	go func() { finished <- server.ListenAndServe(context.Background()) }()
	select {
	case err := <-finished:
		if !errors.Is(err, ErrIdleTimeout) {
			t.Fatalf("idle shutdown error = %v, want %v", err, ErrIdleTimeout)
		}
	case <-time.After(2 * time.Second):
		_ = server.Close()
		t.Fatal("daemon did not stop after idle timeout")
	}
	if _, err := os.Stat(path); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("idle shutdown socket stat error = %v, want not-exist", err)
	}
}

func TestClientAutostartHook(t *testing.T) {
	_, path, _ := startTestServer(t, func(context.Context, Frame) (any, []Warning, error) { return map[string]any{"pong": true}, nil, nil })
	called := false
	client := NewClient(ClientOptions{SocketPath: path, Session: "default", StartDaemon: func(context.Context) error { called = true; return nil }})
	response, err := client.Request(context.Background(), Frame{Cmd: "daemon.ping"})
	if err != nil || !response.Success {
		t.Fatalf("request = %#v, err = %v", response, err)
	}
	if called {
		t.Fatal("autostart hook called while socket was available")
	}
}

func TestHandlerErrorResponsePreservesHardStopMetadata(t *testing.T) {
	hardStop := &session.HardStopError{
		Code:                     session.CodeSessionUserControl,
		Message:                  "human controls session",
		RequiresUserConfirmation: true,
		ResumeHint:               "confirm takeover",
	}
	response := handlerErrorResponse(hardStop)
	if response.Success || response.Error == nil {
		t.Fatalf("response = %#v", response)
	}
	if response.Error.Code != session.CodeSessionUserControl || response.Error.Retryable == nil || *response.Error.Retryable || response.Error.RequiresUserConfirmation == nil || !*response.Error.RequiresUserConfirmation || response.Error.ResumeHint != "confirm takeover" {
		t.Fatalf("hard-stop response = %#v", response.Error)
	}
}

// TestConcurrentStartupYieldsOneOwner guards issue #371: several MCP clients
// recovering the same session at once must not each bind the socket. Before
// the fix every starter unconditionally unlinked the existing socket, so two
// daemons could serve one session with split browser state.
func TestConcurrentStartupYieldsOneOwner(t *testing.T) {
	if !socketOwnershipSupported {
		t.Skip("socket ownership is not decidable on this platform; startup keeps the historical replace behavior")
	}
	// Unix socket paths are length-limited (104 bytes on macOS), so the long
	// t.TempDir() name cannot be used here.
	dir, err := os.MkdirTemp("", "sb-race-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(dir) })
	socketPath := filepath.Join(dir, "race.sock")

	const starters = 6
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	var wg sync.WaitGroup
	var mu sync.Mutex
	servers := make([]*Server, 0, starters)
	results := make(chan error, starters)
	for i := 0; i < starters; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			server := NewServer(Options{
				SocketPath:  socketPath,
				Session:     "race",
				IdleTimeout: -1,
				Handler: func(context.Context, Frame) (any, []Warning, error) {
					return map[string]any{"pong": true}, nil, nil
				},
			})
			mu.Lock()
			servers = append(servers, server)
			mu.Unlock()
			results <- server.ListenAndServe(ctx)
		}()
	}
	// Every starter must be shut down before the test returns, whether it won
	// or lost the race.
	t.Cleanup(func() {
		cancel()
		mu.Lock()
		running := append([]*Server(nil), servers...)
		mu.Unlock()
		for _, server := range running {
			_ = server.Close()
		}
		wg.Wait()
	})

	// Require every loser to return before checking the one surviving endpoint.
	deadline := time.After(10 * time.Second)
	const expectedLosers = starters - 1
	losers := 0
	for losers < expectedLosers {
		select {
		case err := <-results:
			if !errors.Is(err, ErrDaemonAlreadyRunning) {
				t.Fatalf("a starter returned %v, want ErrDaemonAlreadyRunning", err)
			}
			losers++
		case <-deadline:
			t.Fatal("no starter reported ErrDaemonAlreadyRunning; the socket was taken over")
		}
	}

	client := NewClient(ClientOptions{SocketPath: socketPath, Session: "race"})
	response, err := client.RequestWithoutAutostart(context.Background(), Frame{Cmd: "daemon.ping", RequestID: "r1"})
	if err != nil || !response.Success {
		t.Fatalf("ping the surviving daemon: response = %#v, err = %v", response, err)
	}

}

// TestStaleSocketIsReplaced verifies the complement of the single-owner rule:
// a socket file with nothing behind it must not block a fresh daemon.
func TestStaleSocketIsReplaced(t *testing.T) {
	if !socketOwnershipSupported {
		// Without a liveness probe there is nothing platform-specific left to
		// assert here: the socket file is always replaced.
		t.Skip("socket ownership is not decidable on this platform")
	}
	dir, err := os.MkdirTemp("", "sb-stale-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(dir) })
	socketPath := filepath.Join(dir, "stale.sock")
	listener, err := net.Listen("unix", socketPath)
	if err != nil {
		t.Fatal(err)
	}
	// Close the listener but keep the socket file, exactly as an unclean
	// daemon exit leaves it.
	listener.(*net.UnixListener).SetUnlinkOnClose(false)
	if err := listener.Close(); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Lstat(socketPath); err != nil {
		t.Fatalf("stale socket file missing: %v", err)
	}
	server := NewServer(Options{
		SocketPath:  socketPath,
		Session:     "stale",
		IdleTimeout: -1,
		Handler: func(context.Context, Frame) (any, []Warning, error) {
			return map[string]any{"pong": true}, nil, nil
		},
	})
	ctx, cancel := context.WithCancel(context.Background())
	ready := make(chan error, 1)
	go func() { ready <- server.ListenAndServe(ctx) }()
	t.Cleanup(func() {
		cancel()
		_ = server.Close()
		<-ready
	})
	waitForSocket(t, socketPath, ready)
	client := NewClient(ClientOptions{SocketPath: socketPath, Session: "stale", StartDaemon: nil})
	deadline := time.Now().Add(2 * time.Second)
	for {
		response, err := client.RequestWithoutAutostart(context.Background(), Frame{Cmd: "daemon.ping"})
		if err == nil && response.Success {
			break
		}
		if time.Now().After(deadline) {
			t.Fatalf("request after stale-socket recovery = %#v, err = %v", response, err)
		}
		time.Sleep(time.Millisecond)
	}
	stop, err := client.RequestWithoutAutostart(context.Background(), Frame{Cmd: "daemon.stop"})
	if err != nil || !stop.Success {
		t.Fatalf("stop after stale-socket recovery = %#v, err = %v", stop, err)
	}
}
