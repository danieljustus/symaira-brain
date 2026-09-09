//go:build !aix && !darwin && !dragonfly && !freebsd && !hurd && !illumos && !linux && !netbsd && !openbsd && !solaris

package memorytool

import "errors"

const chatGPTFIFOAvailable = false

func makeChatGPTFIFO(string) error {
	return errors.New("named FIFOs are not supported on this platform")
}
