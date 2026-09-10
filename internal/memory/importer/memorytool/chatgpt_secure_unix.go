//go:build aix || darwin || dragonfly || freebsd || linux || netbsd || openbsd || solaris

package memorytool

import (
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strings"

	"golang.org/x/sys/unix"
)

const chatGPTOpenFlags = unix.O_RDONLY | unix.O_CLOEXEC | unix.O_NOFOLLOW | unix.O_NONBLOCK

func discoverChatGPTExports(path string) ([]ExportRef, error) {
	root, err := openChatGPTDirectory(path)
	if err == nil {
		defer root.Close()
		refs := make([]ExportRef, 0)
		seen := 0
		err := walkChatGPTDirectory(root, filepath.Clean(path), 0, &seen, &refs)
		return refs, err
	}
	if !errors.Is(err, unix.ENOTDIR) {
		if errors.Is(err, unix.ELOOP) {
			return nil, nil
		}
		return nil, err
	}

	file, err := openChatGPTFile(path)
	if err != nil {
		if errors.Is(err, unix.ELOOP) {
			return nil, nil
		}
		return nil, err
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() || !strings.HasSuffix(filepath.Base(path), "conversations.json") {
		return nil, nil
	}
	return []ExportRef{{Tool: "chatgpt", Path: path, Format: "json", ModifiedAt: info.ModTime()}}, nil
}

func walkChatGPTDirectory(dir *os.File, path string, depth int, seen *int, refs *[]ExportRef) error {
	remaining := chatGPTDiscoveryMaxEntries - *seen
	if remaining <= 0 {
		return nil
	}
	names, err := dir.Readdirnames(remaining)
	if err != nil && !errors.Is(err, io.EOF) {
		return err
	}
	*seen += len(names)
	sort.Strings(names)
	for _, name := range names {
		entry, err := openChatGPTEntry(dir, name)
		if err != nil {
			// This also skips a symlink substituted between directory read and open.
			continue
		}
		info, statErr := entry.Stat()
		entryPath := filepath.Join(path, name)
		if statErr == nil {
			switch {
			case info.IsDir() && depth < chatGPTDiscoveryMaxDepth:
				_ = walkChatGPTDirectory(entry, entryPath, depth+1, seen, refs)
			case info.Mode().IsRegular() && strings.HasSuffix(name, "conversations.json"):
				*refs = append(*refs, ExportRef{Tool: "chatgpt", Path: entryPath, Format: "json", ModifiedAt: info.ModTime()})
			}
		}
		_ = entry.Close()
	}
	return nil
}

func readChatGPTExport(path string) ([]byte, error) {
	file, err := openChatGPTFile(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() {
		return nil, fmt.Errorf("%s is not a regular file", path)
	}
	if info.Size() > chatGPTMaxExportBytes {
		return nil, fmt.Errorf("%s exceeds maximum size of %d bytes", path, chatGPTMaxExportBytes)
	}
	data, err := io.ReadAll(io.LimitReader(file, chatGPTMaxExportBytes+1))
	if err != nil {
		return nil, err
	}
	if int64(len(data)) > chatGPTMaxExportBytes {
		return nil, fmt.Errorf("%s exceeds maximum size of %d bytes", path, chatGPTMaxExportBytes)
	}
	return data, nil
}

func openChatGPTDirectory(path string) (*os.File, error) {
	clean, err := filepath.Abs(filepath.Clean(path))
	if err != nil {
		return nil, err
	}
	clean = normalizeChatGPTPath(clean)
	fd, err := unix.Open(string(filepath.Separator), unix.O_RDONLY|unix.O_DIRECTORY|unix.O_CLOEXEC|unix.O_NOFOLLOW, 0)
	if err != nil {
		return nil, err
	}
	current := os.NewFile(uintptr(fd), string(filepath.Separator))
	for _, component := range strings.Split(strings.TrimPrefix(clean, string(filepath.Separator)), string(filepath.Separator)) {
		if component == "" || component == "." {
			continue
		}
		if component == ".." {
			_ = current.Close()
			return nil, fmt.Errorf("path contains parent component")
		}
		nextFD, err := unix.Openat(int(current.Fd()), component, unix.O_RDONLY|unix.O_DIRECTORY|unix.O_CLOEXEC|unix.O_NOFOLLOW, 0)
		if err != nil {
			_ = current.Close()
			return nil, err
		}
		next := os.NewFile(uintptr(nextFD), filepath.Join(current.Name(), component))
		_ = current.Close()
		current = next
	}
	return current, nil
}

func openChatGPTFile(path string) (*os.File, error) {
	parent, err := openChatGPTDirectory(filepath.Dir(path))
	if err != nil {
		return nil, err
	}
	defer parent.Close()
	return openChatGPTEntry(parent, filepath.Base(path))
}

func openChatGPTEntry(parent *os.File, name string) (*os.File, error) {
	fd, err := unix.Openat(int(parent.Fd()), name, chatGPTOpenFlags, 0)
	if err != nil {
		return nil, err
	}
	return os.NewFile(uintptr(fd), name), nil
}

func normalizeChatGPTPath(path string) string {
	if runtime.GOOS == "darwin" {
		if path == "/var" || strings.HasPrefix(path, "/var/") {
			return "/private" + path
		}
	}
	return path
}
