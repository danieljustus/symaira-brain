//go:build windows

package daemon

import (
	"context"
	"net"

	"github.com/Microsoft/go-winio"
)

const daemonPipeSecurityDescriptor = "D:P(A;;GA;;;OW)"

func listenDaemonEndpoint(path string) (net.Listener, error) {
	return winio.ListenPipe(path, &winio.PipeConfig{
		SecurityDescriptor: daemonPipeSecurityDescriptor,
	})
}

func secureDaemonEndpoint(string) error { return nil }

func cleanupDaemonEndpoint(string) {}

func dialDaemonEndpoint(ctx context.Context, path string) (net.Conn, error) {
	return winio.DialPipeContext(ctx, path)
}
