//go:build unix

package usage

import (
	"errors"
	"os"
	"path/filepath"
	"syscall"
)

func openCredentialFile(path string) (*os.File, error) {
	root, err := os.OpenRoot(filepath.Dir(path))
	if err != nil {
		return nil, err
	}
	file, err := root.OpenFile(filepath.Base(path), os.O_RDONLY|syscall.O_NOFOLLOW, 0)
	closeErr := root.Close()
	if err != nil {
		return nil, err
	}
	if closeErr != nil {
		if fileCloseErr := file.Close(); fileCloseErr != nil {
			return nil, errors.Join(closeErr, fileCloseErr)
		}
		return nil, closeErr
	}
	return file, nil
}
