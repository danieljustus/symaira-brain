//go:build unix

package usage

import (
	"io"
	"os"
	"syscall"

	"golang.org/x/sys/unix"
)

func readOllamaKeyFile(path string) ([]byte, error) {
	// Nonblocking prevents a substituted FIFO from stalling before fstat.
	fd, err := unix.Open(path, unix.O_RDONLY|unix.O_NOFOLLOW|unix.O_NONBLOCK|unix.O_CLOEXEC, 0)
	if err != nil {
		return nil, ollamaKeyError()
	}
	file := os.NewFile(uintptr(fd), "Ollama signing key")
	defer file.Close()
	info, err := file.Stat()
	if err != nil {
		return nil, ollamaKeyError()
	}
	stat, ok := info.Sys().(*syscall.Stat_t)
	if !ok || stat.Uid != uint32(os.Geteuid()) || stat.Nlink != 1 || !info.Mode().IsRegular() || info.Mode().Perm()&0077 != 0 || info.Mode()&(os.ModeSetuid|os.ModeSetgid|os.ModeSticky) != 0 || info.Size() <= 0 || info.Size() > maxCredentialFileBytes {
		return nil, ollamaKeyError()
	}
	data, err := io.ReadAll(io.LimitReader(file, maxCredentialFileBytes+1))
	if err != nil || len(data) > maxCredentialFileBytes {
		return nil, ollamaKeyError()
	}
	return data, nil
}
