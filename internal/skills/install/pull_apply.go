package install

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"

	"github.com/danieljustus/symaira-brain/internal/skills/fsutil"
	"github.com/danieljustus/symaira-brain/internal/skills/vcs"
)

func ApplyPending(opts PullOptions) error {
	lock, err := AcquirePullLock(opts.Target, opts.Name, opts)
	if err != nil {
		return err
	}
	defer lock.Release()
	stage, err := PendingPath(opts.Target, opts.Name, opts)
	if err != nil {
		return err
	}
	if _, err := os.Stat(filepath.Join(stage, pullManifestFile)); err != nil {
		return fmt.Errorf("no pending pull for %s/%s", opts.Target, opts.Name)
	}
	if opts.LibraryDir == "" {
		return errors.New("apply requires library directory")
	}
	target := filepath.Join(opts.LibraryDir, opts.Name)
	tmp := target + ".pull-tmp-" + fmt.Sprint(os.Getpid())
	if err := os.RemoveAll(tmp); err != nil {
		return err
	}
	defer os.RemoveAll(tmp)
	if err := fsutil.CopyTree(stage, tmp, func(rel string, d os.DirEntry) bool {
		return rel == pullManifestFile || (d.Name() == ".git" && d.IsDir())
	}); err != nil {
		return err
	}
	entries, err := os.ReadDir(target)
	if err != nil {
		return err
	}
	for _, entry := range entries {
		if entry.Name() == ".git" {
			continue
		}
		if err := os.RemoveAll(filepath.Join(target, entry.Name())); err != nil {
			return err
		}
	}
	tmpEntries, err := os.ReadDir(tmp)
	if err != nil {
		return err
	}
	for _, entry := range tmpEntries {
		if err := os.Rename(filepath.Join(tmp, entry.Name()), filepath.Join(target, entry.Name())); err != nil {
			return err
		}
	}
	if vcs.IsRepo(target) {
		if _, err := vcs.Commit(target, fmt.Sprintf("pull: apply pending changes for %s", opts.Name)); err != nil && !errors.Is(err, vcs.ErrUnavailable) {
			return err
		}
	}
	return os.RemoveAll(stage)
}
