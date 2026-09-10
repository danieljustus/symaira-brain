//go:build aix || darwin || dragonfly || freebsd || hurd || illumos || linux || netbsd || openbsd || solaris

package memorytool

import "syscall"

const chatGPTFIFOAvailable = true

func makeChatGPTFIFO(path string) error {
	return syscall.Mkfifo(path, 0o600)
}
