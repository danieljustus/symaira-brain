//go:build unix

package render

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"syscall"
	"time"
)

const (
	defaultRenderLockTimeout = 30 * time.Second
	renderLockWait           = 10 * time.Millisecond
	renderLockMaxWait        = 100 * time.Millisecond
)

// renderLockTimeout is a variable so lock-contention tests can use a short
// deterministic deadline. Production acquisition remains bounded at 30s.
var renderLockTimeout = defaultRenderLockTimeout

// lockDestination takes a bounded, stable advisory lock shared by goroutines
// and processes. The lock lives beside, never inside, the destination tree.
func lockDestination(root *os.Root, parent, destination string) (func(), error) {
	lockPath := filepath.Join(parent, ".symskills-lock-"+filepath.Base(destination))
	deadline := time.Now().Add(renderLockTimeout)
	var file *os.File
	var err error
	for attempt := 0; ; attempt++ {
		file, err = root.OpenFile(lockPath, os.O_CREATE|os.O_RDWR|syscall.O_NOFOLLOW, 0o600)
		if err == nil {
			break
		}
		if !errors.Is(err, os.ErrNotExist) {
			return nil, fmt.Errorf("open render lock: %w", err)
		}
		if !waitForRenderLock(deadline, attempt) {
			return nil, fmt.Errorf("timed out acquiring render lock for %q", destination)
		}
	}
	for attempt := 0; ; attempt++ {
		err = syscall.Flock(int(file.Fd()), syscall.LOCK_EX|syscall.LOCK_NB)
		if err == nil {
			return func() {
				_ = syscall.Flock(int(file.Fd()), syscall.LOCK_UN)
				_ = file.Close()
			}, nil
		}
		if !errors.Is(err, syscall.EWOULDBLOCK) && !errors.Is(err, syscall.EAGAIN) {
			_ = file.Close()
			return nil, fmt.Errorf("lock destination: %w", err)
		}
		if !waitForRenderLock(deadline, attempt) {
			_ = file.Close()
			return nil, fmt.Errorf("timed out acquiring render lock for %q", destination)
		}
	}
}

func waitForRenderLock(deadline time.Time, attempt int) bool {
	remaining := time.Until(deadline)
	if remaining <= 0 {
		return false
	}
	delay := renderLockWait
	for i := 0; i < attempt && delay < renderLockMaxWait; i++ {
		delay *= 2
	}
	if delay > renderLockMaxWait {
		delay = renderLockMaxWait
	}
	if delay > remaining {
		delay = remaining
	}
	time.Sleep(delay)
	return true
}
