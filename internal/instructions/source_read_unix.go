//go:build darwin || dragonfly || freebsd || linux || netbsd || openbsd || solaris

package instructions

import (
	"fmt"
	"os"
	"path/filepath"
)

func readSourceFile(path string) ([]byte, error) {
	parent, err := openSourceParent(path)
	if err != nil {
		return nil, err
	}
	defer parent.Close()
	name := filepath.Base(path)
	info, err := parent.Lstat(name)
	if err != nil {
		return nil, err
	}
	if info.mode&os.ModeSymlink != 0 || !info.mode.IsRegular() {
		return nil, fmt.Errorf("%s is not a regular file", path)
	}
	if info.size > MaxSourceFileBytes {
		return nil, fmt.Errorf("%s exceeds maximum size of %d bytes", path, MaxSourceFileBytes)
	}
	file, err := parent.OpenRead(name)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	return readBoundedFile(file, path)
}
