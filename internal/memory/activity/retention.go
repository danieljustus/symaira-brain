package activity

import (
	"database/sql"
	"fmt"
	"os"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

// Expire deletes rows whose configured expiry has passed. The operation is
// transactional and then checkpoints/truncates SQLite's WAL and vacuums the
// shared database before returning, so expiry is an on-disk deletion rather
// than merely a hidden row.
func (s *Store) Expire(at ...time.Time) (RetentionResult, error) {
	now := s.now()
	if len(at) > 0 && !at[0].IsZero() {
		now = at[0].UTC()
	}
	tx, err := s.db.BeginTransaction()
	if err != nil {
		return RetentionResult{}, err
	}
	var result RetentionResult
	if count, err := deleteCount(tx, "DELETE FROM activity_episodes WHERE expires_at <= ?", now); err != nil {
		_ = tx.Rollback()
		return RetentionResult{}, err
	} else {
		result.Episodes = count
	}
	if count, err := deleteCount(tx, "DELETE FROM activity_segments WHERE expires_at <= ?", now); err != nil {
		_ = tx.Rollback()
		return RetentionResult{}, err
	} else {
		result.Segments = count
	}
	if err := tx.Commit(); err != nil {
		return RetentionResult{}, err
	}
	if result.Segments > 0 || result.Episodes > 0 {
		if err := s.finalizeDeletion(); err != nil {
			return result, err
		}
	}
	return result, nil
}

// PurgeExpired is a descriptive alias for Expire.
func (s *Store) PurgeExpired(at ...time.Time) (RetentionResult, error) {
	return s.Expire(at...)
}

// DeleteEpisode removes an episode and all segments named by its provenance
// list. It verifies that those IDs no longer exist after WAL checkpointing and
// vacuuming.
func (s *Store) DeleteEpisode(id string) (RetentionResult, error) {
	episode, err := s.GetEpisode(id)
	if err != nil {
		return RetentionResult{}, err
	}
	if episode == nil {
		return RetentionResult{}, nil
	}
	tx, err := s.db.BeginTransaction()
	if err != nil {
		return RetentionResult{}, err
	}
	result := RetentionResult{}
	if count, err := deleteCount(tx, "DELETE FROM activity_episodes WHERE id = ?", id); err != nil {
		_ = tx.Rollback()
		return RetentionResult{}, err
	} else {
		result.Episodes = count
	}
	for _, segmentID := range episode.SegmentIDs {
		if count, err := deleteCount(tx, "DELETE FROM activity_segments WHERE id = ?", segmentID); err != nil {
			_ = tx.Rollback()
			return RetentionResult{}, err
		} else {
			result.Segments += count
		}
	}
	if err := tx.Commit(); err != nil {
		return RetentionResult{}, err
	}
	if err := s.finalizeDeletion(); err != nil {
		return result, err
	}
	if err := s.verifyIDsAbsent(append([]string{id}, episode.SegmentIDs...)); err != nil {
		return result, err
	}
	return result, nil
}

// ClearEpisode is an alias for DeleteEpisode.
func (s *Store) ClearEpisode(id string) (RetentionResult, error) {
	return s.DeleteEpisode(id)
}

// ClearTimeRange removes every episode and segment overlapping [start, end).
// Episodes are removed first so their provenance cannot keep a deleted segment
// reachable through an episode row.
func (s *Store) ClearTimeRange(start, end time.Time) (RetentionResult, error) {
	if end.IsZero() || !end.After(start) {
		return RetentionResult{}, fmt.Errorf("activity clear range must be increasing")
	}
	tx, err := s.db.BeginTransaction()
	if err != nil {
		return RetentionResult{}, err
	}
	var result RetentionResult
	if count, err := deleteCount(tx, `DELETE FROM activity_episodes
		WHERE ended_at > ? AND started_at < ?`, start.UTC(), end.UTC()); err != nil {
		_ = tx.Rollback()
		return RetentionResult{}, err
	} else {
		result.Episodes = count
	}
	if count, err := deleteCount(tx, `DELETE FROM activity_segments
		WHERE ended_at > ? AND started_at < ?`, start.UTC(), end.UTC()); err != nil {
		_ = tx.Rollback()
		return RetentionResult{}, err
	} else {
		result.Segments = count
	}
	if err := tx.Commit(); err != nil {
		return RetentionResult{}, err
	}
	if err := s.finalizeDeletion(); err != nil {
		return result, err
	}
	return result, nil
}

// ClearAll removes all activity rows and verifies that the database has no
// activity WAL residue. Durable memories, evidence, and other tables are not
// touched.
func (s *Store) ClearAll() (RetentionResult, error) {
	tx, err := s.db.BeginTransaction()
	if err != nil {
		return RetentionResult{}, err
	}
	var result RetentionResult
	if count, err := deleteCount(tx, "DELETE FROM activity_episodes"); err != nil {
		_ = tx.Rollback()
		return RetentionResult{}, err
	} else {
		result.Episodes = count
	}
	if count, err := deleteCount(tx, "DELETE FROM activity_segments"); err != nil {
		_ = tx.Rollback()
		return RetentionResult{}, err
	} else {
		result.Segments = count
	}
	if err := tx.Commit(); err != nil {
		return RetentionResult{}, err
	}
	if err := s.finalizeDeletion(); err != nil {
		return result, err
	}
	return result, nil
}

// ClearAllActivity is an explicit alias for ClearAll.
func (s *Store) ClearAllActivity() (RetentionResult, error) { return s.ClearAll() }

func deleteCount(execer db.SQLExecer, query string, args ...any) (int, error) {
	result, err := execer.Exec(query, args...)
	if err != nil {
		return 0, err
	}
	count, err := result.RowsAffected()
	if err != nil {
		return 0, nil
	}
	return int(count), nil
}

// ClearLastSession deletes the latest activity episode and its segment
// provenance. A missing episode is a successful no-op.
func (s *Store) ClearLastSession() (RetentionResult, error) {
	var id string
	err := s.db.Conn().QueryRow(`SELECT id FROM activity_episodes ORDER BY ended_at DESC, id DESC LIMIT 1`).Scan(&id)
	if err == sql.ErrNoRows {
		return RetentionResult{}, nil
	}
	if err != nil {
		return RetentionResult{}, err
	}
	return s.DeleteEpisode(id)
}

func (s *Store) finalizeDeletion() error {
	if _, err := s.db.Conn().Exec("PRAGMA secure_delete = ON"); err != nil {
		return fmt.Errorf("enable secure activity deletion: %w", err)
	}
	if _, err := s.db.Conn().Exec("PRAGMA wal_checkpoint(TRUNCATE)"); err != nil {
		return fmt.Errorf("checkpoint activity WAL: %w", err)
	}
	if _, err := s.db.Conn().Exec("VACUUM"); err != nil {
		return fmt.Errorf("vacuum activity database: %w", err)
	}
	if _, err := s.db.Conn().Exec("PRAGMA wal_checkpoint(TRUNCATE)"); err != nil {
		return fmt.Errorf("checkpoint activity WAL after vacuum: %w", err)
	}
	return s.VerifyDiskClean()
}

// VerifyDiskClean checks that a completed deletion left no pending WAL pages.
// SQLite's secure_delete plus VACUUM do the byte erasure; this check makes the
// on-disk checkpoint part observable to callers and tests.
func (s *Store) VerifyDiskClean() error {
	path := s.db.Path()
	if path == "" {
		return nil
	}
	walPath := path + "-wal"
	if info, err := os.Stat(walPath); err == nil && info.Size() != 0 {
		return fmt.Errorf("activity deletion left %d bytes in SQLite WAL", info.Size())
	} else if err != nil && !os.IsNotExist(err) {
		return fmt.Errorf("inspect SQLite WAL: %w", err)
	}
	return nil
}

func (s *Store) verifyIDsAbsent(ids []string) error {
	for _, id := range ids {
		var count int
		if err := s.db.Conn().QueryRow(`SELECT COUNT(*) FROM activity_segments WHERE id = ?`, id).Scan(&count); err != nil {
			return err
		}
		if count != 0 {
			return fmt.Errorf("activity segment %s still exists after deletion", id)
		}
		if err := s.db.Conn().QueryRow(`SELECT COUNT(*) FROM activity_episodes WHERE id = ?`, id).Scan(&count); err != nil {
			return err
		}
		if count != 0 {
			return fmt.Errorf("activity episode %s still exists after deletion", id)
		}
	}
	return nil
}
