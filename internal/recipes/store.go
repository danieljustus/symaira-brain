package recipes

import (
	"bufio"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"sync"
)

// maxEpisodesPerStore bounds one profile's episode file. Episodes are
// append-only behavioral history; the bound keeps the file cheap to
// read on every promotion query and prunes the oldest history first.
const maxEpisodesPerStore = 1000

// Store persists episodes as JSONL, one Episode per line, for a single
// profile. It is safe for concurrent use and never fails on a malformed
// line when loading (the line is skipped — history is best-effort).
type Store struct {
	path string
	mu   sync.Mutex
}

// NewStore returns a Store writing to path (typically
// <XDG data dir>/recipes/<profile>.jsonl). The file is created lazily
// on the first Append.
func NewStore(path string) *Store {
	return &Store{path: path}
}

// Append writes one episode and prunes the file to the newest
// maxEpisodesPerStore episodes when it exceeds the bound.
func (s *Store) Append(ep Episode) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	if err := os.MkdirAll(filepath.Dir(s.path), 0o755); err != nil {
		return err
	}

	data, err := json.Marshal(ep)
	if err != nil {
		return err
	}

	f, err := os.OpenFile(s.path, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o644)
	if err != nil {
		return err
	}
	if _, err := f.Write(append(data, '\n')); err != nil {
		f.Close()
		return err
	}
	if err := f.Close(); err != nil {
		return err
	}

	return s.pruneLocked()
}

// Load returns every episode currently in the store. A missing file
// yields an empty slice; malformed lines are skipped (never a hard
// error), matching the best-effort nature of behavioral history.
func (s *Store) Load() ([]Episode, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	f, err := os.Open(s.path)
	if errors.Is(err, os.ErrNotExist) {
		return []Episode{}, nil
	}
	if err != nil {
		return nil, err
	}
	defer f.Close()

	var episodes []Episode
	scanner := bufio.NewScanner(f)
	scanner.Buffer(make([]byte, 0, 64*1024), 1<<20)
	for scanner.Scan() {
		var ep Episode
		if err := json.Unmarshal(scanner.Bytes(), &ep); err != nil {
			continue
		}
		episodes = append(episodes, ep)
	}
	if err := scanner.Err(); err != nil {
		return nil, err
	}
	return episodes, nil
}

// pruneLocked rewrites the file keeping only the newest
// maxEpisodesPerStore episodes. Callers must hold s.mu.
func (s *Store) pruneLocked() error {
	episodes, err := s.loadLocked()
	if err != nil {
		return err
	}
	if len(episodes) <= maxEpisodesPerStore {
		return nil
	}

	keep := episodes[len(episodes)-maxEpisodesPerStore:]
	tmp := s.path + ".tmp"
	f, err := os.Create(tmp)
	if err != nil {
		return err
	}
	for _, ep := range keep {
		data, err := json.Marshal(ep)
		if err != nil {
			f.Close()
			os.Remove(tmp)
			return err
		}
		if _, err := f.Write(append(data, '\n')); err != nil {
			f.Close()
			os.Remove(tmp)
			return err
		}
	}
	if err := f.Close(); err != nil {
		os.Remove(tmp)
		return err
	}
	return os.Rename(tmp, s.path)
}

// loadLocked is Load without locking; callers must hold s.mu.
func (s *Store) loadLocked() ([]Episode, error) {
	f, err := os.Open(s.path)
	if errors.Is(err, os.ErrNotExist) {
		return []Episode{}, nil
	}
	if err != nil {
		return nil, err
	}
	defer f.Close()

	var episodes []Episode
	scanner := bufio.NewScanner(f)
	scanner.Buffer(make([]byte, 0, 64*1024), 1<<20)
	for scanner.Scan() {
		var ep Episode
		if err := json.Unmarshal(scanner.Bytes(), &ep); err != nil {
			continue
		}
		episodes = append(episodes, ep)
	}
	return episodes, scanner.Err()
}
