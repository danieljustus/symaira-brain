//go:build !darwin && !linux

package daemon

import (
	"net"
)

// validatePeerUID is a best-effort hardening check on platforms that expose
// peer credentials (Linux SO_PEERCRED, macOS LOCAL_PEERCRED). Windows uses
// an owner-only named-pipe ACL when creating the daemon endpoint.
func validatePeerUID(net.Conn) error {
	return nil
}
