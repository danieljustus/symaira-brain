//go:build !aix && !darwin && !dragonfly && !freebsd && !linux && !netbsd && !openbsd && !solaris

package memorytool

import (
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

func discoverChatGPTExports(path string) ([]ExportRef, error) {
	info, err := os.Lstat(path)
	if err != nil {
		return nil, err
	}
	if info.Mode()&os.ModeSymlink != 0 || (!info.IsDir() && !info.Mode().IsRegular()) {
		return nil, nil
	}
	if !info.IsDir() {
		if strings.HasSuffix(filepath.Base(path), "conversations.json") {
			return []ExportRef{{Tool: "chatgpt", Path: path, Format: "json", ModifiedAt: info.ModTime()}}, nil
		}
		return nil, nil
	}

	dir, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer dir.Close()
	refs := make([]ExportRef, 0)
	seen := 0
	return refs, walkChatGPTDirectory(dir, filepath.Clean(path), 0, &seen, &refs)
}

func walkChatGPTDirectory(dir *os.File, path string, depth int, seen *int, refs *[]ExportRef) error {
	remaining := chatGPTDiscoveryMaxEntries - *seen
	if remaining <= 0 {
		return nil
	}
	names, err := dir.Readdirnames(remaining)
	if err != nil && err != io.EOF {
		return err
	}
	*seen += len(names)
	sort.Strings(names)
	for _, name := range names {
		entryPath := filepath.Join(path, name)
		info, err := os.Lstat(entryPath)
		if err != nil || info.Mode()&os.ModeSymlink != 0 {
			continue
		}
		if info.IsDir() {
			if depth >= chatGPTDiscoveryMaxDepth {
				continue
			}
			entry, err := os.Open(entryPath)
			if err == nil {
				_ = walkChatGPTDirectory(entry, entryPath, depth+1, seen, refs)
				_ = entry.Close()
			}
			continue
		}
		if !info.Mode().IsRegular() || !strings.HasSuffix(name, "conversations.json") {
			continue
		}
		entry, err := os.Open(entryPath)
		if err != nil {
			continue
		}
		entryInfo, statErr := entry.Stat()
		_ = entry.Close()
		if statErr == nil && entryInfo.Mode().IsRegular() {
			*refs = append(*refs, ExportRef{Tool: "chatgpt", Path: entryPath, Format: "json", ModifiedAt: entryInfo.ModTime()})
		}
	}
	return nil
}

func readChatGPTExport(path string) ([]byte, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() {
		return nil, os.ErrInvalid
	}
	if info.Size() > chatGPTMaxExportBytes {
		return nil, &os.PathError{Op: "read", Path: path, Err: os.ErrInvalid}
	}
	data, err := io.ReadAll(io.LimitReader(file, chatGPTMaxExportBytes+1))
	if err != nil {
		return nil, err
	}
	if int64(len(data)) > chatGPTMaxExportBytes {
		return nil, &os.PathError{Op: "read", Path: path, Err: os.ErrInvalid}
	}
	return data, nil
}
