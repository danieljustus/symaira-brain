package aging

import (
	"database/sql"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

func TestDecayFactorCurve(t *testing.T) {
	cfg := DefaultConfig()
	now := time.Now().UTC()
	created := now.AddDate(0, 0, -10)

	cases := []struct {
		name       string
		lastAccess *time.Time
		access     int64
		wantMin    float64
		wantMax    float64
	}{
		{"never accessed", nil, 0, 0.9, 1.0}, // 10 days old → barely decayed
		{"accessed recently", &now, 5, 0.95, 1.0},
		{"stale unused", ptr(now.AddDate(0, 0, -400)), 0, 0.02, 0.15},  // ~400 days → deep decay
		{"stale but used", ptr(now.AddDate(0, 0, -400)), 25, 0.5, 0.9}, // access boost holds it up
	}
	for _, tc := range cases {
		got := DecayFactor(cfg, created, tc.lastAccess, tc.access, now)
		if got < tc.wantMin || got > tc.wantMax {
			t.Errorf("%s: decay = %v, want in [%v, %v]", tc.name, got, tc.wantMin, tc.wantMax)
		}
	}
}

func TestDecayFactorDisabledReturnsOne(t *testing.T) {
	cfg := DefaultConfig()
	cfg.Enabled = false
	old := time.Now().UTC().AddDate(0, 0, -2000)
	if got := DecayFactor(cfg, old, nil, 0, time.Now().UTC()); got != 1.0 {
		t.Fatalf("disabled aging must yield factor 1.0, got %v", got)
	}
}

func TestDecayFactorClampsAndBounds(t *testing.T) {
	cfg := DefaultConfig()
	now := time.Now().UTC()
	// Ancient, never accessed → clamped at the 0.02 floor, never zero/negative.
	got := DecayFactor(cfg, now.AddDate(0, 0, -5000), nil, 0, now)
	if got < 0.02 || got > 1.0 {
		t.Fatalf("decay out of bounds: %v", got)
	}
	// Future timestamps must not boost above 1.0.
	got = DecayFactor(cfg, now.AddDate(0, 0, 100), ptr(now.AddDate(0, 0, 100)), 100, now)
	if got > 1.0 {
		t.Fatalf("future-dated memory boosted above 1.0: %v", got)
	}
}

func TestRunDryRunAndReal(t *testing.T) {
	database := openAgingTestDB(t)
	now := time.Now().UTC()

	// Fresh memory: no decay change.
	fresh := &db.Memory{ID: "fresh", Content: "fresh fact", Scope: "global", Metadata: map[string]string{}, CreatedAt: now.AddDate(0, 0, -2)}
	// Stale memory: should decay and eventually retire under an aggressive curve.
	stale := &db.Memory{ID: "stale", Content: "stale fact", Scope: "global", Metadata: map[string]string{}, CreatedAt: now.AddDate(0, 0, -600)}
	for _, m := range []*db.Memory{fresh, stale} {
		if err := database.SaveMemory(m); err != nil {
			t.Fatal(err)
		}
	}

	cfg := DefaultConfig()
	cfg.AccessHalfLifeDays = 120
	cfg.RetireBelow = 0.1

	// Dry run: counts what would happen, writes nothing.
	report, err := Run(database, cfg, now, true)
	if err != nil {
		t.Fatal(err)
	}
	if !report.DryRun {
		t.Fatal("dry-run report must set DryRun")
	}
	if report.Scanned != 2 {
		t.Fatalf("dry run scanned %d, want 2", report.Scanned)
	}
	if report.Decayed != 1 || report.Retired != 1 {
		t.Fatalf("dry run: decayed=%d retired=%d, want 1/1", report.Decayed, report.Retired)
	}

	// Nothing written yet. Note: GetMemory tracks access (last_access),
	// which would change the aging inputs — read the raw row instead.
	var retiredAt sql.NullTime
	var decayFactor float64
	if err := database.Conn().QueryRow("SELECT retired_at, decay_factor FROM memories WHERE id = ?", "stale").Scan(&retiredAt, &decayFactor); err != nil {
		t.Fatal(err)
	}
	if retiredAt.Valid || decayFactor != 1.0 {
		t.Fatal("dry run must not write")
	}

	// Real run: writes decay and retirement.
	report, err = Run(database, cfg, now, false)
	if err != nil {
		t.Fatal(err)
	}
	if report.DryRun {
		t.Fatal("real run report must not set DryRun")
	}
	if report.Retired != 1 || report.Decayed != 1 {
		t.Fatalf("real run: decayed=%d retired=%d, want 1/1", report.Decayed, report.Retired)
	}
	if len(report.RetiredIDs) != 1 || report.RetiredIDs[0] != "stale" {
		t.Fatalf("retired ids = %v, want [stale]", report.RetiredIDs)
	}

	// Second run is a no-op (nothing left to decay). Run it before any
	// GetMemory call: GetMemory tracks access and would change the inputs.
	report, err = Run(database, cfg, now, false)
	if err != nil {
		t.Fatal(err)
	}
	if report.Decayed != 0 || report.Retired != 0 {
		t.Fatalf("second run must be a no-op, got decayed=%d retired=%d", report.Decayed, report.Retired)
	}

	// Row assertions via raw SQL (GetMemory would track access).
	var staleRetired sql.NullTime
	var freshDecay float64
	if err := database.Conn().QueryRow("SELECT retired_at FROM memories WHERE id = ?", "stale").Scan(&staleRetired); err != nil {
		t.Fatal(err)
	}
	if !staleRetired.Valid {
		t.Fatal("stale memory not flagged retired after real run")
	}
	if err := database.Conn().QueryRow("SELECT decay_factor FROM memories WHERE id = ?", "fresh").Scan(&freshDecay); err != nil {
		t.Fatal(err)
	}
	if freshDecay >= 1.0 || freshDecay < 0.9 {
		t.Fatalf("fresh memory should carry a slight decay (0.9..1.0), got %v", freshDecay)
	}
}

func TestRunDisabledIsNoOp(t *testing.T) {
	database := openAgingTestDB(t)
	cfg := DefaultConfig()
	cfg.Enabled = false
	report, err := Run(database, cfg, time.Now().UTC(), false)
	if err != nil {
		t.Fatal(err)
	}
	if report.Scanned != 0 || report.Decayed != 0 || report.Retired != 0 {
		t.Fatalf("disabled run must be a no-op: %+v", report)
	}
}

func TestReportString(t *testing.T) {
	r := &Report{Scanned: 5, Decayed: 2, Retired: 1, DryRun: true, RetiredIDs: []string{"x"}}
	if s := r.String(); s == "" || !strings.Contains(s, "dry-run") {
		t.Fatalf("dry-run report string missing marker: %q", s)
	}
	r.DryRun = false
	if s := r.String(); !strings.Contains(s, "retired 1") {
		t.Fatalf("report string missing count: %q", s)
	}
}

func ptr(t time.Time) *time.Time { return &t }

func openAgingTestDB(t *testing.T) *db.DB {
	t.Helper()
	tempDir, err := os.MkdirTemp("", "symmemory-aging-test-*")
	if err != nil {
		t.Fatalf("temp dir: %v", err)
	}
	t.Cleanup(func() { os.RemoveAll(tempDir) })
	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	t.Cleanup(func() { os.Setenv("HOME", oldHome) })

	database, err := db.Open(config.Defaults())
	if err != nil {
		t.Fatalf("open db: %v", err)
	}
	t.Cleanup(func() { _ = database.Close() })
	return database
}
