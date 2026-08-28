package db

import (
	"database/sql"
	"fmt"
	"os"
	"sync"
	"sync/atomic"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/paths"
	"github.com/danieljustus/symaira-corekit/sqlitekit"
	_ "modernc.org/sqlite"
)

// DB wraps the SQL connection.
type DB struct {
	conn                  *sql.DB
	quantizeBinary        bool // store sign-bit binary vectors on save
	prefilterEnabled      bool // use Hamming prefilter before cosine scoring
	sparsemaxEnabled      bool // apply sparsemax (α=2) to fused hybrid scores
	perArmMultiplier      int  // per-arm result cap multiplier before fusion
	retrievalStats        *RetrievalStats
	hydrationMetrics      *hydrationMetrics
	queryLogMaxEntries    int           // query_log row cap (default 1000)
	queryLogMaxAge        time.Duration // query_log max entry age; 0 = disabled
	queryLogRecordResults atomic.Bool   // record returned memory ids per query (issue #460)

	// spreadSeedOnce guards the one-time auto-seeding of association edges
	// when the spreading term is enabled (#488).
	spreadSeedOnce sync.Once
}

// Open initializes the SQLite database at the standard XDG path,
// or at the path specified in the supplied configuration. The caller
// (typically cmd/) is responsible for loading configuration via
// config.Load(); library code never reads from disk directly.
func Open(cfg *config.Config) (*DB, error) {
	if cfg == nil {
		cfg = config.Defaults()
	}

	var dbPath string
	if cfg.Database.Path != "" {
		dbPath = cfg.Database.Path
	} else {
		var err error
		dbPath, err = paths.DatabasePath()
		if err != nil {
			return nil, fmt.Errorf("failed to resolve database path: %w", err)
		}
	}

	conn, err := sqlitekit.Open(dbPath)
	if err != nil {
		return nil, fmt.Errorf("failed to open sqlite database: %w", err)
	}

	conn.SetMaxOpenConns(1)
	conn.SetMaxIdleConns(1)
	conn.SetConnMaxLifetime(0)

	queryLogMaxEntries := cfg.QueryLog.MaxEntries
	if queryLogMaxEntries <= 0 {
		queryLogMaxEntries = maxQueryLogEntries
	}
	var queryLogMaxAge time.Duration
	if cfg.QueryLog.MaxAge != "" {
		if d, err := time.ParseDuration(cfg.QueryLog.MaxAge); err == nil {
			queryLogMaxAge = d
		}
	}

	db := &DB{
		conn:               conn,
		quantizeBinary:     cfg.HybridSearch.QuantizeToBinary,
		prefilterEnabled:   cfg.HybridSearch.PrefilterEnabled,
		sparsemaxEnabled:   cfg.HybridSearch.SparsemaxEnabled,
		perArmMultiplier:   cfg.HybridSearch.PerArmMultiplier,
		retrievalStats:     &RetrievalStats{},
		hydrationMetrics:   &hydrationMetrics{},
		queryLogMaxEntries: queryLogMaxEntries,
		queryLogMaxAge:     queryLogMaxAge,
	}
	db.queryLogRecordResults.Store(cfg.QueryLog.RecordResults)
	if err := db.runMigrations(); err != nil {
		_ = conn.Close()
		return nil, fmt.Errorf("failed to run migrations: %w", err)
	}

	if _, err := os.Stat(dbPath); err == nil {
		if err := os.Chmod(dbPath, 0600); err != nil {
			_ = conn.Close()
			return nil, fmt.Errorf("failed to set db file permissions: %w", err)
		}
	}
	for _, suffix := range []string{"-wal", "-shm"} {
		sibling := dbPath + suffix
		if _, err := os.Stat(sibling); err == nil {
			_ = os.Chmod(sibling, 0600)
		}
	}

	return db, nil
}

// ResolvePath returns the filesystem path to the SQLite database file
// for the given configuration. When cfg is nil or cfg.Database.Path is
// empty the standard XDG default is used. The file may or may not exist.
func ResolvePath(cfg *config.Config) string {
	if cfg != nil && cfg.Database.Path != "" {
		return cfg.Database.Path
	}
	path, err := paths.DatabasePath()
	if err != nil {
		return ""
	}
	return path
}

// RetrievalStats returns the live retrieval stats accumulator.
func (db *DB) RetrievalStats() *RetrievalStats {
	return db.retrievalStats
}

// Close closes the database connection.
func (db *DB) Close() error {
	return db.conn.Close()
}

// Conn returns the underlying SQL connection.
func (db *DB) Conn() *sql.DB {
	return db.conn
}

// BeginTransaction starts a new database transaction.
func (db *DB) BeginTransaction() (*sql.Tx, error) {
	return db.conn.Begin()
}

// Metrics returns current connection pool metrics.
func (db *DB) Metrics() DBMetrics {
	stats := db.conn.Stats()
	return DBMetrics{
		MaxOpenConnections: stats.MaxOpenConnections,
		OpenConnections:    stats.OpenConnections,
		InUse:              stats.InUse,
		Idle:               stats.Idle,
		WaitCount:          stats.WaitCount,
		WaitDuration:       stats.WaitDuration,
		MaxIdleClosed:      stats.MaxIdleClosed,
		MaxIdleTimeClosed:  stats.MaxIdleTimeClosed,
		MaxLifetimeClosed:  stats.MaxLifetimeClosed,
	}
}

// DBMetrics holds connection pool metrics.
type DBMetrics struct {
	MaxOpenConnections int
	OpenConnections    int
	InUse              int
	Idle               int
	WaitCount          int64
	WaitDuration       time.Duration
	MaxIdleClosed      int64
	MaxIdleTimeClosed  int64
	MaxLifetimeClosed  int64
}

// SQLExecer is an interface for executing SQL statements, satisfied by both *sql.DB and *sql.Tx.
type SQLExecer interface {
	Exec(query string, args ...interface{}) (sql.Result, error)
}
