//go:build !unix

package render

import (
	"os"
	"path/filepath"
)

// Native no-follow/reparse handling remains platform-specific. Keep the
// fallback buildable while the Unix capability implementation is authoritative.
func openDestinationRootSecure(path string) (*os.Root, error) {
	if err := ensureRealDirectory(path); err != nil {
		return nil, err
	}
	return os.OpenRoot(path)
}

func openRenderDirNoFollow(root *os.Root, rel string) (*os.Root, error) {
	return root.OpenRoot(filepath.Clean(rel))
}

func openRenderFileNoFollow(root *os.Root, path string) (*os.File, error) {
	return root.OpenFile(path, os.O_RDONLY, 0)
}
