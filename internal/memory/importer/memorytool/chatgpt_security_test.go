package memorytool

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestChatGPTImportExport_SizeBoundary(t *testing.T) {
	for _, tc := range []struct {
		name string
		size int64
		want bool
	}{
		{name: "exact bound", size: chatGPTMaxExportBytes, want: true},
		{name: "one over", size: chatGPTMaxExportBytes + 1, want: false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "conversations.json")
			writePaddedChatGPTJSON(t, path, tc.size)
			_, err := NewChatGPTImporter().ImportExport(ExportRef{Path: path})
			if tc.want && err != nil {
				t.Fatalf("exact-bound export rejected: %v", err)
			}
			if !tc.want && (err == nil || !strings.Contains(err.Error(), "exceeds maximum size")) {
				t.Fatalf("one-over export error = %v, want size error", err)
			}
		})
	}
}

func writePaddedChatGPTJSON(t *testing.T, path string, size int64) {
	t.Helper()
	file, err := os.Create(path)
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	if size < 2 {
		t.Fatalf("invalid JSON test size %d", size)
	}
	if _, err := file.WriteString("[]"); err != nil {
		t.Fatal(err)
	}
	padding := strings.Repeat(" ", 32*1024)
	for remaining := size - 2; remaining > 0; {
		chunk := int64(len(padding))
		if chunk > remaining {
			chunk = remaining
		}
		if _, err := file.WriteString(padding[:chunk]); err != nil {
			t.Fatal(err)
		}
		remaining -= chunk
	}
}

func TestChatGPTImportExport_RejectsSymlinkAndSpecialFiles(t *testing.T) {
	root := t.TempDir()
	target := filepath.Join(root, "conversations.json")
	if err := os.WriteFile(target, []byte("[]"), 0o600); err != nil {
		t.Fatal(err)
	}

	link := filepath.Join(root, "link.json")
	if err := os.Symlink(target, link); err != nil {
		t.Skipf("symlinks unavailable: %v", err)
	}
	if _, err := NewChatGPTImporter().ImportExport(ExportRef{Path: link}); err == nil {
		t.Fatal("ImportExport followed a symlink")
	}

	if !chatGPTFIFOAvailable {
		return
	}
	fifo := filepath.Join(root, "fifo.json")
	if err := makeChatGPTFIFO(fifo); err != nil {
		t.Fatal(err)
	}
	finished := make(chan error, 1)
	go func() {
		_, err := NewChatGPTImporter().ImportExport(ExportRef{Path: fifo})
		finished <- err
	}()
	select {
	case err := <-finished:
		if err == nil {
			t.Fatal("ImportExport accepted a FIFO")
		}
	case <-time.After(2 * time.Second):
		t.Fatal("ImportExport blocked on a FIFO")
	}
}

func TestChatGPTImportExport_RejectsDevice(t *testing.T) {
	if _, err := os.Stat("/dev/null"); err != nil {
		t.Skip("device file unavailable")
	}
	if _, err := NewChatGPTImporter().ImportExport(ExportRef{Path: "/dev/null"}); err == nil {
		t.Fatal("ImportExport accepted a device")
	}
}
