//go:build !windows

package render

import "os"

func syncDirectoryHandle(dir *os.File) error {
	return dir.Sync()
}
