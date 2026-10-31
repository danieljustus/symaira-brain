//go:build unix

package usage

import (
	"errors"
	"io"
	"os"
	"path/filepath"
)

// The older shared reader omits NONBLOCK; keep its frozen web oracle intact.
func openCodeReadAuthFile(path string) (data []byte, err error) {
	root, err := openCodeDirectory(filepath.Dir(path))
	if err != nil {
		return nil, err
	}
	defer func() { err = errors.Join(err, root.Close()) }()
	return openCodeDBFile(root, filepath.Base(path), maxCredentialFileBytes)
}
func openCodeReadDir(path string) (entries []os.DirEntry, err error) {
	file, err := openCodeDirectory(path)
	if err != nil {
		return nil, err
	}
	defer func() { err = errors.Join(err, file.Close()) }()
	entries, err = file.ReadDir(257)
	if errors.Is(err, io.EOF) {
		err = nil
	}
	if len(entries) > 256 {
		return nil, os.ErrInvalid
	}
	return entries, err
}
