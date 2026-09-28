package main

import (
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"sync"
	"testing"
)

// TestCreateProfileFileNoClobber pins #461: concurrent creators of the same
// profile publish exactly once and never replace each other's file.
func TestCreateProfileFileNoClobber(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "race.toml")
	const creators = 8
	errs := make([]error, creators)
	var wg sync.WaitGroup
	for i := range creators {
		wg.Add(1)
		go func() {
			defer wg.Done()
			errs[i] = createProfileFile(path, []byte{byte('a' + i)})
		}()
	}
	wg.Wait()

	winner := -1
	for i, err := range errs {
		switch {
		case err == nil:
			if winner != -1 {
				t.Fatalf("creators %d and %d both succeeded", winner, i)
			}
			winner = i
		case !errors.Is(err, os.ErrExist):
			t.Fatalf("creator %d: %v", i, err)
		}
	}
	if winner == -1 {
		t.Fatal("no creator succeeded")
	}
	data, err := os.ReadFile(path)
	if err != nil || string(data) != string(rune('a'+winner)) {
		t.Fatalf("profile = %q, %v; want winner %d", data, err, winner)
	}
	info, err := os.Stat(path)
	if err != nil || (runtime.GOOS != "windows" && info.Mode().Perm() != 0o600) {
		t.Fatalf("mode = %v, %v", info.Mode(), err)
	}
	entries, _ := os.ReadDir(dir)
	if len(entries) != 1 {
		t.Fatalf("leftover temp files: %v", entries)
	}
}
