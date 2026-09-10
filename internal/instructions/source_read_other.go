//go:build !darwin && !dragonfly && !freebsd && !linux && !netbsd && !openbsd && !solaris && !windows

package instructions

import "fmt"

// Platforms without a no-follow open primitive fail closed rather than
// falling back to a path re-open that could race a symlink replacement.
func readSourceFile(path string) ([]byte, error) {
	return nil, fmt.Errorf("instruction source reads are unsupported on this platform: %s", path)
}
