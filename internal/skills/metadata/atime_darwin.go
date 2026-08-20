//go:build darwin

package metadata

import (
	"os"
	"syscall"
	"time"
)

// accessTime returns the file's access time from the stat buffer.
func accessTime(fi os.FileInfo) time.Time {
	st, ok := fi.Sys().(*syscall.Stat_t)
	if !ok {
		return time.Time{}
	}
	return time.Unix(st.Atimespec.Sec, st.Atimespec.Nsec)
}
