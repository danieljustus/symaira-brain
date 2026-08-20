//go:build !linux && !darwin

package metadata

import (
	"os"
	"time"
)

// accessTime is not available on this platform; the atime signal is simply
// never reported. last_used then degrades to null rather than a guess.
func accessTime(os.FileInfo) time.Time {
	return time.Time{}
}
