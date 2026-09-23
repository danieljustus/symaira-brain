//go:build windows

package render

import "os"

// Windows does not support FlushFileBuffers on a directory handle. The caller
// still opens the directory through the rooted no-follow path before reaching
// this platform-specific durability boundary; file contents are synced
// separately before staged trees are renamed.
func syncDirectoryHandle(_ *os.File) error {
	return nil
}
