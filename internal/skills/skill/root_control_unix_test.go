//go:build unix

package skill

import (
	"errors"
	"net"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"golang.org/x/sys/unix"
)

func TestControlOpenRetainsCapabilityAfterRenameAndReplacement(t *testing.T) {
	for _, name := range []string{"SKILL.md", "symskills.toml"} {
		t.Run(name, func(t *testing.T) {
			owned := t.TempDir()
			original := filepath.Join(owned, "original")
			writeFile(t, filepath.Join(original, name), "held bytes")
			root, err := openBundleRoot(original)
			if err != nil {
				t.Fatal(err)
			}
			defer root.Close()
			renamed := filepath.Join(owned, "renamed")
			if err := os.Rename(original, renamed); err != nil {
				t.Fatal(err)
			}
			writeFile(t, filepath.Join(original, name), "replacement bytes")
			data, err := readRootFile(root, name, name, MaxInputSize)
			if err != nil || string(data) != "held bytes" {
				t.Fatalf("retained capability: %q, %v", data, err)
			}
			if err := os.Rename(filepath.Join(renamed, name), filepath.Join(renamed, "document")); err != nil {
				t.Fatal(err)
			}
			if err := os.Symlink("document", filepath.Join(renamed, name)); err != nil {
				t.Fatal(err)
			}
			data, err = readRootFile(root, name, name, MaxInputSize)
			var pathError *os.PathError
			if len(data) != 0 || !errors.Is(err, unix.ELOOP) || !errors.As(err, &pathError) || pathError.Op != "openat" || pathError.Path != name {
				t.Fatalf("final control link must fail at held root: %q, %v", data, err)
			}
			if err.Error() != "read "+name+": openat "+name+": "+unix.ELOOP.Error() {
				t.Fatalf("control diagnostic must wrap once: %v", err)
			}
		})
	}
}

func TestControlOpenRejectsEveryFinalLinkWithoutTargetRead(t *testing.T) {
	for _, name := range []string{"SKILL.md", "symskills.toml"} {
		for _, kind := range []string{"file", "directory", "outside", "dangling", "loop", "fifo", "socket", "device"} {
			t.Run(name+"/"+kind, func(t *testing.T) {
				owned := t.TempDir()
				bundlePath := filepath.Join(owned, "bundle")
				if err := os.Mkdir(bundlePath, 0o700); err != nil {
					t.Fatal(err)
				}
				writeMinimalSkill(t, bundlePath)
				target := "target"
				switch kind {
				case "file":
					writeFile(t, filepath.Join(bundlePath, target), "never read")
				case "directory":
					if err := os.Mkdir(filepath.Join(bundlePath, target), 0o700); err != nil {
						t.Fatal(err)
					}
				case "outside":
					target = "../outside"
					writeFile(t, filepath.Join(owned, "outside"), "outside bytes")
				case "dangling":
					target = "absent"
				case "loop":
					target = name
				case "fifo":
					if err := unix.Mkfifo(filepath.Join(bundlePath, target), 0o600); err != nil {
						t.Fatal(err)
					}
				case "socket":
					// The configured external TMPDIR may exceed sockaddr_un's limit.
					// These subtests are sequential; keep the short path inside owned.
					t.Chdir(owned)
					listener, err := net.Listen("unix", "s")
					if err != nil {
						t.Fatal(err)
					}
					defer listener.Close()
					target = "../s"
				case "device":
					target = "/dev/null"
				}
				if name == "SKILL.md" {
					if err := os.Remove(filepath.Join(bundlePath, name)); err != nil {
						t.Fatal(err)
					}
				}
				if err := os.Symlink(target, filepath.Join(bundlePath, name)); err != nil {
					t.Fatal(err)
				}
				if _, err := LoadBundle(bundlePath); !errors.Is(err, unix.ELOOP) {
					t.Fatalf("%s link must be refused before body read: %v", kind, err)
				}
			})
		}
	}
}

func TestControlOpenBasenamesRegularityAndBounds(t *testing.T) {
	bundlePath := t.TempDir()
	root, err := openBundleRoot(bundlePath)
	if err != nil {
		t.Fatal(err)
	}
	defer root.Close()
	for _, name := range []string{"", ".", "..", "../SKILL.md", "sub/SKILL.md", "/SKILL.md", "other", "SKILL.md\x00"} {
		file, err := openControlFile(root, name)
		if file != nil || !errors.Is(err, os.ErrInvalid) {
			t.Fatalf("non-basename/control accepted: %q, %v", name, err)
		}
	}
	for _, name := range []string{"SKILL.md", "symskills.toml"} {
		writeFile(t, filepath.Join(bundlePath, name), "12345")
		if data, err := readRootFile(root, name, name, 4); len(data) != 0 || err == nil || !strings.Contains(err.Error(), "maximum input size") {
			t.Fatalf("bounded same-handle read: %q, %v", data, err)
		}
		if err := os.Remove(filepath.Join(bundlePath, name)); err != nil {
			t.Fatal(err)
		}
		if err := unix.Mkfifo(filepath.Join(bundlePath, name), 0o600); err != nil {
			t.Fatal(err)
		}
		if data, err := readRootFile(root, name, name, MaxInputSize); len(data) != 0 || err == nil || err.Error() != name+" must be a regular file" {
			t.Fatalf("nonblocking regularity guard: %q, %v", data, err)
		}
	}
}
