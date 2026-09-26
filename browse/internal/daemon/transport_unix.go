//go:build !windows

package daemon

import (
	"context"
	"net"
	"os"
	"path/filepath"
)

func prepareEndpoint(path string) error {
	return prepareSocketDir(filepath.Dir(path))
}

func secureEndpoint(path string) error {
	return os.Chmod(path, 0o600)
}

func cleanupEndpoint(listener net.Listener, path string) {
	_ = listener.Close()
	_ = os.Remove(path)
}

func listenEndpoint(path string) (net.Listener, error) {
	return net.Listen("unix", path)
}

func dialEndpoint(ctx context.Context, path string) (net.Conn, error) {
	return (&net.Dialer{}).DialContext(ctx, "unix", path)
}

func windowsSocketPathIn(base, session string) string {
	return filepath.Join(base, session+".sock")
}
