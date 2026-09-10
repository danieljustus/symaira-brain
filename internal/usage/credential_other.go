//go:build !unix

package usage

import (
	"errors"
	"os"
	"path/filepath"
)

func openCredentialFile(path string) (*os.File, error) {
	root, err := os.OpenRoot(filepath.Dir(path))
	if err != nil {
		return nil, err
	}
	file, err := root.Open(filepath.Base(path))
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
