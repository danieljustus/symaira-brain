//go:build !(aix || darwin || dragonfly || freebsd || linux || netbsd || openbsd || solaris || windows)

package profile

import "os"

// Remove removes the named profile on platforms without a handle-relative
// filesystem API. Unix and Windows builds provide no-follow implementations.
func Remove(name string) error {
	return os.Remove(Path(name))
}
