//go:build windows

package main

import (
	"context"
	"net"
	"time"

	winio "github.com/Microsoft/go-winio"
)

func listenTestEndpoint(path string) (net.Listener, func(), error) {
	listener, err := winio.ListenPipe(path, &winio.PipeConfig{SecurityDescriptor: `D:P(A;;GA;;;OW)`})
	if err != nil {
		return nil, nil, err
	}
	return listener, func() { _ = listener.Close() }, nil
}

func dialTestEndpoint(path string) (net.Conn, error) {
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	return winio.DialPipeContext(ctx, path)
}
