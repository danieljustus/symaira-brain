//go:build windows

package daemon

import (
	"context"
	"io"
	"net"
	"testing"
	"time"
)

func TestWindowsNamedPipeMatchesRustEndpointAndFraming(t *testing.T) {
	path, err := SocketPath("native-contract")
	if err != nil {
		t.Fatal(err)
	}
	if want := `\\.\pipe\symbrowse-native-contract`; path != want {
		t.Fatalf("SocketPath() = %q, want %q", path, want)
	}
	path, err = SocketPathIn(t.TempDir(), "native-contract")
	if err != nil {
		t.Fatal(err)
	}
	listener, err := listenEndpoint(path)
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	if _, err := listenEndpoint(path); err != ErrDaemonAlreadyRunning {
		t.Fatalf("second listener error = %v, want ErrDaemonAlreadyRunning", err)
	}

	accepted := make(chan net.Conn, 1)
	acceptErr := make(chan error, 1)
	go func() {
		conn, err := listener.Accept()
		if err != nil {
			acceptErr <- err
			return
		}
		accepted <- conn
	}()
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	client, err := dialEndpoint(ctx, path)
	if err != nil {
		t.Fatal(err)
	}
	defer client.Close()
	var server net.Conn
	select {
	case server = <-accepted:
	case err := <-acceptErr:
		t.Fatal(err)
	case <-ctx.Done():
		t.Fatal("named pipe connection was not accepted")
	}
	defer server.Close()
	_ = client.SetDeadline(time.Now().Add(2 * time.Second))
	_ = server.SetDeadline(time.Now().Add(2 * time.Second))
	if _, err := io.WriteString(client, "request\n"); err != nil {
		t.Fatal(err)
	}
	line := make([]byte, len("request\n"))
	if _, err := io.ReadFull(server, line); err != nil {
		t.Fatal(err)
	}
	if string(line) != "request\n" {
		t.Fatalf("frame bytes = %q", line)
	}
}

func TestWindowsNamedPipeServerObservesCancelAndIdleTimeout(t *testing.T) {
	t.Run("cancel", func(t *testing.T) {
		path, err := SocketPathIn(t.TempDir(), "cancel")
		if err != nil {
			t.Fatal(err)
		}
		server := NewServer(Options{SocketPath: path, Session: "cancel", IdleTimeout: time.Minute})
		ctx, cancel := context.WithCancel(context.Background())
		done := make(chan error, 1)
		go func() { done <- server.ListenAndServe(ctx) }()
		waitForNamedPipe(t, path)
		cancel()
		select {
		case err := <-done:
			if err != nil {
				t.Fatal(err)
			}
		case <-time.After(2 * time.Second):
			t.Fatal("ListenAndServe did not observe cancellation")
		}
	})

	t.Run("idle", func(t *testing.T) {
		path, err := SocketPathIn(t.TempDir(), "idle")
		if err != nil {
			t.Fatal(err)
		}
		server := NewServer(Options{SocketPath: path, Session: "idle", IdleTimeout: 75 * time.Millisecond})
		done := make(chan error, 1)
		go func() { done <- server.ListenAndServe(context.Background()) }()
		select {
		case err := <-done:
			if err != ErrIdleTimeout {
				t.Fatalf("ListenAndServe() = %v, want ErrIdleTimeout", err)
			}
		case <-time.After(2 * time.Second):
			t.Fatal("ListenAndServe did not observe idle timeout")
		}
	})
}

func waitForNamedPipe(t *testing.T, path string) {
	t.Helper()
	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		ctx, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
		conn, err := dialEndpoint(ctx, path)
		cancel()
		if err == nil {
			_ = conn.Close()
			return
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatalf("named pipe %q did not become ready", path)
}
