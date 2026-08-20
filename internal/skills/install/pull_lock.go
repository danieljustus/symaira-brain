package install

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"time"

	"github.com/danieljustus/symaira-brain/internal/skills/render"
)

// PullLockStaleAfter is the maximum age of a lockfile before a later run may
// recover it. A lock held by a live or same-process run is never treated as
// stale before this timeout.
const PullLockStaleAfter = 10 * time.Minute

type pullLockRecord struct {
	PID       int    `json:"pid"`
	CreatedAt string `json:"created_at"`
}

// PullLock serializes mutations for one target/skill pair.
type PullLock struct {
	path string
}

// AcquirePullLock obtains the per-target/skill lock. It uses O_EXCL so two
// processes cannot both enter the critical section. Old lockfiles are
// recoverable after PullLockStaleAfter.
func AcquirePullLock(target render.Target, name string, opts PullOptions) (*PullLock, error) {
	pending, err := PendingPath(target, name, opts)
	if err != nil {
		return nil, err
	}
	path := filepath.Join(filepath.Dir(filepath.Dir(pending)), ".locks", string(target), name+".lock")
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return nil, err
	}
	record := pullLockRecord{PID: os.Getpid(), CreatedAt: time.Now().UTC().Format(time.RFC3339Nano)}
	data, _ := json.Marshal(record)
	for attempt := 0; attempt < 2; attempt++ {
		f, err := os.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o644)
		if err == nil {
			if _, werr := f.Write(append(data, '\n')); werr != nil {
				_ = f.Close()
				_ = os.Remove(path)
				return nil, werr
			}
			if cerr := f.Close(); cerr != nil {
				_ = os.Remove(path)
				return nil, cerr
			}
			return &PullLock{path: path}, nil
		}
		if !errors.Is(err, os.ErrExist) || attempt == 1 {
			return nil, fmt.Errorf("pull lock held for %s/%s (%s)", target, name, path)
		}
		info, serr := os.Stat(path)
		if serr == nil && time.Since(info.ModTime()) > PullLockStaleAfter {
			if rerr := os.Remove(path); rerr == nil {
				continue
			}
		}
		return nil, fmt.Errorf("pull lock held for %s/%s (%s)", target, name, path)
	}
	return nil, fmt.Errorf("pull lock held for %s/%s (%s)", target, name, path)
}

// Release removes the lockfile. It is idempotent.
func (l *PullLock) Release() error {
	if l == nil || l.path == "" {
		return nil
	}
	if err := os.Remove(l.path); errors.Is(err, os.ErrNotExist) {
		return nil
	} else {
		return err
	}
}
