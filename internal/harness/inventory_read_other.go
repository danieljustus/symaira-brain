//go:build !(aix || darwin || dragonfly || freebsd || linux || netbsd || openbsd || solaris)

package harness

import (
	"os"

	"github.com/danieljustus/symaira-brain/internal/safefs"
)

func openConfigFile(path string) (*os.File, error) {
	return safefs.OpenConfigFile(path)
}
