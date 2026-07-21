package harness

import (
	"fmt"
	"os"
	"time"

	"github.com/danieljustus/symaira-corekit/fsutil"
)

// backupTimeFormat renders the timestamp used in *.bak.<timestamp> backup
// filenames: sortable and filesystem-safe on every platform symbrain
// targets (no colons).
const backupTimeFormat = "20060102T150405Z"

// Backup copies the file at path to "path.bak.<timestamp>" (UTC) before
// symbrain writes to it, so a botched install/uninstall can always be
// undone by hand. The copy is written atomically via
// corekit/fsutil.AtomicWriteFile and preserves path's original file mode.
//
// If path does not exist yet, Backup is a no-op: it returns an empty
// string and a nil error, since there is nothing to back up before
// creating a brand-new config file.
func Backup(path string) (string, error) {
	info, err := os.Stat(path)
	if err != nil {
		if os.IsNotExist(err) {
			return "", nil
		}
		return "", fmt.Errorf("harness: stat %s: %w", path, err)
	}

	data, err := os.ReadFile(path)
	if err != nil {
		return "", fmt.Errorf("harness: read %s: %w", path, err)
	}

	backupPath := path + ".bak." + time.Now().UTC().Format(backupTimeFormat)
	if err := fsutil.AtomicWriteFile(backupPath, data, info.Mode().Perm()); err != nil {
		return "", fmt.Errorf("harness: write backup %s: %w", backupPath, err)
	}
	return backupPath, nil
}
