//go:build !unix

package render

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
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

// lockDestination uses an anchored create-new lock file on platforms without
// Unix advisory locks. The root capability keeps the lock path below the
// already-validated destination parent and prevents reparse escapes.
func lockDestination(root *os.Root, parent, destination string) (func(), error) {
	lockPath := filepath.Join(parent, ".symskills-lock-"+filepath.Base(destination))
	deadline := time.Now().Add(renderLockTimeout)
	for attempt := 0; ; attempt++ {
		file, err := root.OpenFile(lockPath, os.O_CREATE|os.O_EXCL|os.O_RDWR, 0o600)
		if err == nil {
			return func() {
				_ = file.Close()
				_ = root.Remove(lockPath)
			}, nil
		}
		if !errors.Is(err, os.ErrExist) {
			return nil, fmt.Errorf("open render lock: %w", err)
		}
		if !waitForRenderLock(deadline, attempt) {
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
