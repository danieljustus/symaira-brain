//go:build unix

package usage

import (
	"os"
	"path/filepath"
	"syscall"
	"testing"
)

func TestOpenCodeAPIAuthRejectsLinksAndFIFOs(t *testing.T) {
	root := t.TempDir()
	dir := filepath.Join(root, "data")
	if err := os.Mkdir(dir, 0700); err != nil {
		t.Fatal(err)
	}
	sentinel := filepath.Join(root, "outside.json")
	if err := os.WriteFile(sentinel, []byte(`{"opencode-go":{"key":"outside-sentinel"}}`), 0600); err != nil {
		t.Fatal(err)
	}
	auth := filepath.Join(dir, "auth.json")
	if err := os.Symlink(sentinel, auth); err != nil {
		t.Fatal(err)
	}
	if key, err := openCodeAuthJSONKey(dir); err == nil || key != "" {
		t.Fatal("followed auth symlink")
	}
	if err := os.Remove(auth); err != nil {
		t.Fatal(err)
	}
	if err := syscall.Mkfifo(auth, 0600); err != nil {
		t.Fatal(err)
	}
	if key, err := openCodeAuthJSONKey(dir); err == nil || key != "" {
		t.Fatal("accepted auth fifo")
	}
	fifoDir := filepath.Join(root, "not-a-directory")
	if err := syscall.Mkfifo(fifoDir, 0600); err != nil {
		t.Fatal(err)
	}
	if key, err := openCodeAuthJSONKey(fifoDir); err == nil || key != "" {
		t.Fatal("accepted fifo data directory")
	}
}
