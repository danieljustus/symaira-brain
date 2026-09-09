package usage

import (
	"io"
	"os"
)

const maxCredentialFileBytes = 64 << 10

// readCredentialFile reads a bounded regular file through a capability-rooted
// opener. Platform implementations reject symlinked final path components.
func readCredentialFile(path string) (data []byte, err error) {
	file, err := openCredentialFile(path)
	if err != nil {
		return nil, err
	}
	defer func() {
		if closeErr := file.Close(); err == nil {
			err = closeErr
		}
	}()
	info, err := file.Stat()
	if err != nil || !info.Mode().IsRegular() || info.Size() > maxCredentialFileBytes {
		return nil, os.ErrInvalid
	}
	data, err = io.ReadAll(io.LimitReader(file, maxCredentialFileBytes+1))
	if err != nil {
		return nil, err
	}
	if len(data) > maxCredentialFileBytes {
		return nil, os.ErrInvalid
	}
	return data, nil
}
