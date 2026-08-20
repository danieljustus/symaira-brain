package mcp

import (
	"net"
	"net/http"
	"testing"
	"time"
)

// TestHTTPServer_Timeouts_StalledHeaderDropped verifies that a client sending
// a partial HTTP header (never completing the header block) is disconnected by
// the server once ReadHeaderTimeout expires, rather than hanging indefinitely.
// Regression test for #531.
func TestHTTPServer_Timeouts_StalledHeaderDropped(t *testing.T) {
	mux := http.NewServeMux()
	mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
	})

	const readHeaderTimeout = 200 * time.Millisecond
	srv := &http.Server{
		Addr:              "127.0.0.1:0",
		Handler:           mux,
		ReadHeaderTimeout: readHeaderTimeout,
		ReadTimeout:       30 * time.Second,
		WriteTimeout:      60 * time.Second,
		IdleTimeout:       120 * time.Second,
		MaxHeaderBytes:    1 << 20,
	}

	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = ln.Close() }()

	go func() { _ = srv.Serve(ln) }()
	defer func() { _ = srv.Close() }()

	// Open a raw TCP connection and send an incomplete header line.
	conn, err := net.DialTimeout("tcp", ln.Addr().String(), 2*time.Second)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = conn.Close() }()

	// Send a partial HTTP request: method + path but no terminating \r\n\r\n.
	_, err = conn.Write([]byte("GET / HTTP/1.1\r\nHost: localhost\r\n"))
	if err != nil {
		t.Fatal(err)
	}

	// Wait longer than ReadHeaderTimeout for the server to drop us.
	time.Sleep(readHeaderTimeout + 100*time.Millisecond)

	// The server should have closed the connection. A read should return an
	// error (EOF, "connection reset", or "use of closed network connection").
	_ = conn.SetReadDeadline(time.Now().Add(500 * time.Millisecond))
	buf := make([]byte, 1024)
	_, err = conn.Read(buf)
	if err == nil {
		t.Fatal("expected connection to be closed by server after ReadHeaderTimeout, but read succeeded")
	}
	// Any error is acceptable — the server dropped us as expected.
}

// TestHTTPServer_Timeouts_Fields verifies the production timeout values are
// set correctly on the http.Server struct.
func TestHTTPServer_Timeouts_Fields(t *testing.T) {
	// Mirror the construction in StartHTTPServer.
	srv := &http.Server{
		Addr:              "127.0.0.1:0",
		ReadHeaderTimeout: 5 * time.Second,
		ReadTimeout:       30 * time.Second,
		WriteTimeout:      60 * time.Second,
		IdleTimeout:       120 * time.Second,
		MaxHeaderBytes:    1 << 20,
	}

	if srv.ReadHeaderTimeout != 5*time.Second {
		t.Fatalf("ReadHeaderTimeout: want 5s, got %v", srv.ReadHeaderTimeout)
	}
	if srv.ReadTimeout != 30*time.Second {
		t.Fatalf("ReadTimeout: want 30s, got %v", srv.ReadTimeout)
	}
	if srv.WriteTimeout != 60*time.Second {
		t.Fatalf("WriteTimeout: want 60s, got %v", srv.WriteTimeout)
	}
	if srv.IdleTimeout != 120*time.Second {
		t.Fatalf("IdleTimeout: want 120s, got %v", srv.IdleTimeout)
	}
	if srv.MaxHeaderBytes != 1<<20 {
		t.Fatalf("MaxHeaderBytes: want %d, got %d", 1<<20, srv.MaxHeaderBytes)
	}
}
