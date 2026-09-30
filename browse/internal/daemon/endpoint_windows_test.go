//go:build windows

package daemon

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestSocketPathUsesBrowseNamedPipe(t *testing.T) {
	path, err := SocketPath("test-session")
	if err != nil {
		t.Fatal(err)
	}
	if want := `\\.\pipe\symbrowse-test-session`; path != want {
		t.Fatalf("SocketPath() = %q, want %q", path, want)
	}
}

func TestWindowsNamedPipeContractMatchesRust(t *testing.T) {
	root := filepath.Join("..", "..")
	spec, err := os.ReadFile(filepath.Join(root, "crates", "symbrowse-daemon", "src", "spec.rs"))
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(spec), `r"\\.\pipe\symbrowse-{session}"`) {
		t.Fatal("Rust default socket path no longer declares the symbrowse named-pipe convention")
	}
	server, err := os.ReadFile(filepath.Join(root, "crates", "symbrowse-daemon", "src", "server.rs"))
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(server), `u16cstr!("D:P(A;;GA;;;OW)")`) {
		t.Fatal("Rust Windows named-pipe owner-only ACL changed; update the Go transport contract")
	}
	if daemonPipeSecurityDescriptor != "D:P(A;;GA;;;OW)" {
		t.Fatalf("Go pipe ACL = %q, want Rust owner-only ACL", daemonPipeSecurityDescriptor)
	}
}

func TestWindowsNamedPipeDaemonRoundTrip(t *testing.T) {
	session := fmt.Sprintf("test-%x", time.Now().UnixNano())
	path, err := SocketPath(session)
	if err != nil {
		t.Fatal(err)
	}
	server := NewServer(Options{
		SocketPath:  path,
		Session:     session,
		IdleTimeout: -1,
		Handler: func(_ context.Context, frame Frame) (any, []Warning, error) {
			if frame.Cmd != "daemon.ping" {
				return nil, nil, fmt.Errorf("unexpected command %q", frame.Cmd)
			}
			return map[string]bool{"pong": true}, nil, nil
		},
	})
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	done := make(chan error, 1)
	go func() { done <- server.ListenAndServe(ctx) }()

	client := NewClient(ClientOptions{SocketPath: path, Session: session})
	deadline := time.Now().Add(5 * time.Second)
	var response Response
	var requestErr error
	for time.Now().Before(deadline) {
		response, requestErr = client.RequestWithoutAutostart(context.Background(), Frame{Cmd: "daemon.ping"})
		if requestErr == nil {
			break
		}
		time.Sleep(10 * time.Millisecond)
	}
	if requestErr != nil {
		t.Fatalf("named-pipe daemon request failed: %v", requestErr)
	}
	data, ok := response.Data.(map[string]any)
	if !response.Success || !ok || data["pong"] != true {
		t.Fatalf("unexpected daemon response: %+v", response)
	}

	cancel()
	select {
	case err := <-done:
		if err != nil {
			t.Fatalf("ListenAndServe() = %v", err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("named-pipe daemon did not stop after context cancellation")
	}
}

func TestWindowsNamedPipeDaemonHonorsIdleTimeout(t *testing.T) {
	session := fmt.Sprintf("idle-%x", time.Now().UnixNano())
	path, err := SocketPath(session)
	if err != nil {
		t.Fatal(err)
	}
	server := NewServer(Options{SocketPath: path, Session: session, IdleTimeout: 50 * time.Millisecond})
	done := make(chan error, 1)
	go func() { done <- server.ListenAndServe(context.Background()) }()
	select {
	case err := <-done:
		if err != ErrIdleTimeout {
			t.Fatalf("ListenAndServe() = %v, want ErrIdleTimeout", err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("named-pipe daemon did not stop at its idle timeout")
	}
}
