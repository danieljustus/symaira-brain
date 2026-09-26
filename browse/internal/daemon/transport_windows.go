//go:build windows

package daemon

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"net"
	"syscall"

	winio "github.com/Microsoft/go-winio"
)

func prepareEndpoint(string) error { return nil }

func secureEndpoint(string) error { return nil }

func cleanupEndpoint(listener net.Listener, _ string) {
	_ = listener.Close()
}

func listenEndpoint(path string) (net.Listener, error) {
	// Match the Rust daemon's owner-only DACL. go-winio also rejects remote
	// clients and creates the first pipe instance exclusively.
	listener, err := winio.ListenPipe(path, &winio.PipeConfig{SecurityDescriptor: `D:P(A;;GA;;;OW)`})
	if errors.Is(err, syscall.ERROR_ACCESS_DENIED) {
		// Rust's interprocess listener reports duplicate first-instance
		// ownership as ErrDaemonAlreadyRunning on Windows too.
		return nil, ErrDaemonAlreadyRunning
	}
	return listener, err
}

func dialEndpoint(ctx context.Context, path string) (net.Conn, error) {
	return winio.DialPipeContext(ctx, path)
}

func windowsSocketPathIn(base, session string) string {
	baseID := sha256.Sum256([]byte(base))
	return `\\.\pipe\symbrowse-test-` + hex.EncodeToString(baseID[:8]) + `-` + session
}
