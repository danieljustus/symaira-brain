//go:build unix

package usage

import (
	"bytes"
	"context"
	"errors"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"golang.org/x/sys/unix"
)

func readHistoryFiles(ctx context.Context, path string) ([]historyEvent, int, error) {
	return readJSONHistoryFiles(ctx, path, false, parseHistoryLine)
}

func readJSONHistoryFiles(ctx context.Context, path string, claude bool, parse func([]byte) ([]historyEvent, bool)) ([]historyEvent, int, error) {
	return readJSONHistoryFilesReset(ctx, path, claude, parse, func(string) bool { return true })
}

func readJSONHistoryFilesReset(ctx context.Context, path string, claude bool, parse func([]byte) ([]historyEvent, bool), reset func(string) bool) ([]historyEvent, int, error) {
	root, err := openCodeDirectory(path)
	if errors.Is(err, os.ErrNotExist) {
		return nil, 0, nil
	}
	if err != nil {
		return nil, 0, err
	}
	defer root.Close()
	deadline := time.Now().Add(2 * time.Second)
	var events []historyEvent
	excluded, entries, files, total, lines := 0, 0, 0, 0, 0
	var walk func(*os.File, int, string) error
	walk = func(dir *os.File, depth int, relative string) error {
		if depth > 12 || ctx.Err() != nil || time.Now().After(deadline) {
			return os.ErrInvalid
		}
		children, err := dir.ReadDir(4097 - entries)
		if err != nil && !errors.Is(err, io.EOF) {
			return err
		}
		entries += len(children)
		if entries > 4096 {
			return os.ErrInvalid
		}
		sort.Slice(children, func(i, j int) bool { return children[i].Name() < children[j].Name() })
		for _, child := range children {
			if ctx.Err() != nil || time.Now().After(deadline) {
				return os.ErrInvalid
			}
			if child.Type()&os.ModeSymlink != 0 {
				return os.ErrInvalid
			}
			if child.IsDir() {
				fd, err := unix.Openat(int(dir.Fd()), child.Name(), unix.O_RDONLY|unix.O_DIRECTORY|unix.O_NOFOLLOW|unix.O_NONBLOCK|unix.O_CLOEXEC, 0)
				if err != nil {
					return err
				}
				sub := os.NewFile(uintptr(fd), "history directory")
				err = walk(sub, depth+1, filepath.Join(relative, child.Name()))
				closeErr := sub.Close()
				if err != nil {
					return err
				}
				if closeErr != nil {
					return closeErr
				}
			} else if (!claude && child.Name() == "updates.jsonl") || (claude && strings.HasSuffix(child.Name(), ".jsonl")) {
				if !reset(filepath.Join(relative, child.Name())) {
					continue
				}
				files++
				if files > 128 {
					return os.ErrInvalid
				}
				data, err := openCodeDBFile(dir, child.Name(), 4<<20)
				if err != nil {
					return err
				}
				total += len(data)
				if total > 32<<20 {
					return os.ErrInvalid
				}
				records := bytes.Split(data, []byte{'\n'})
				if len(records[len(records)-1]) > 0 {
					excluded++
				}
				for _, line := range records[:len(records)-1] {
					if len(bytes.TrimSpace(line)) == 0 {
						continue
					}
					lines++
					if lines > 20000 || len(line) > 256<<10 || ctx.Err() != nil || time.Now().After(deadline) {
						return os.ErrInvalid
					}
					parsed, ok := parse(line)
					if !ok {
						excluded++
					}
					events = append(events, parsed...)
					if len(events) > 10000 {
						return os.ErrInvalid
					}
				}
			}
		}
		return nil
	}
	err = walk(root, 0, "")
	return events, excluded, err
}
