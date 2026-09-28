//go:build !windows

package daemon

import (
	"context"
	"net"
	"os"
)

func listenDaemonEndpoint(path string) (net.Listener, error) {
	return net.Listen("unix", path)
}

func secureDaemonEndpoint(path string) error {
	return os.Chmod(path, 0o600)
}

func cleanupDaemonEndpoint(path string) { _ = os.Remove(path) }

func dialDaemonEndpoint(ctx context.Context, path string) (net.Conn, error) {
	return (&net.Dialer{}).DialContext(ctx, "unix", path)
}
