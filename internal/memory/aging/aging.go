// Package aging implements explicit memory aging (#491): a per-fact decay
// factor derived from last access, access count, and creation time, stored
// as a column and multiplied into the retrieval score. Facts whose decay
// drops below a configurable floor are retired by flagging — never
// hard-deleted. The pass runs alongside the consolidation engine, not
// inside it.
package aging

import (
	"fmt"
	"math"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

// Config controls the aging pass. All fields have safe defaults via
// DefaultConfig; a factor of 1.0 disables decay for a store.
type Config struct {
	Enabled            bool    // master switch; when false the pass is a no-op
	AccessHalfLifeDays float64 // days after which an unaccessed fact halves its decay
	RetireBelow        float64 // decay below this floor retires the memory (flagged, not deleted)
	AccessBoostCap     int64   // access count at which the access boost saturates
}

// DefaultConfig returns the default aging policy.
//
// The curve is deliberately conservative: a fact that was never accessed
// loses half its retrieval weight every AccessHalfLifeDays (120) and is
// retired after roughly a year of silence (decay < 0.1), while a fact that
// is accessed regularly never decays below ~0.75 and is never retired.
// Every number is overridable and decay can be pinned to 1.0 to disable.
func DefaultConfig() Config {
	return Config{
		Enabled:            true,
		AccessHalfLifeDays: 120,
		RetireBelow:        0.1,
		AccessBoostCap:     20,
	}
}

// FromConfig maps the persisted configuration section onto the pass config.
func FromConfig(cfg config.AgingConfig) Config {
	return Config{
		Enabled:            cfg.Enabled,
		AccessHalfLifeDays: cfg.AccessHalfLifeDays,
		RetireBelow:        cfg.RetireBelow,
		AccessBoostCap:     cfg.AccessBoostCap,
	}
}

// DecayFactor computes the aging multiplier for one memory at time now.
//
//	decay = clamp(recency * (1 - 0.5*boost) + 0.5*boost, 0.02, 1.0)
//
// where recency is exponential decay over the days since the effective
// last access (the real last_access, falling back to created_at for facts
// that were never retrieved) and boost is the log-scaled access count
// (saturating at AccessBoostCap). A heavily accessed fact therefore
// decays much less than a fact nobody ever asked for again.
func DecayFactor(cfg Config, createdAt time.Time, lastAccess *time.Time, accessCount int64, now time.Time) float64 {
	if !cfg.Enabled {
		return 1.0
	}
	halfLife := cfg.AccessHalfLifeDays
	if halfLife <= 0 {
		halfLife = 120
	}
	cap := cfg.AccessBoostCap
	if cap <= 0 {
		cap = 20
	}

	effective := createdAt
	if lastAccess != nil && !lastAccess.IsZero() {
		effective = *lastAccess
	}
	days := now.Sub(effective).Hours() / 24.0
	if days < 0 {
		days = 0
	}
	recency := math.Exp(-0.693 * days / halfLife)

	// The access boost requires evidence of access: a row whose
	// last_access was never recorded has never been retrieved, even if the
	// legacy save path defaulted access_count to 1.
	boost := 0.0
	if lastAccess != nil && !lastAccess.IsZero() {
		boost = math.Log1p(float64(accessCount)) / math.Log1p(float64(cap))
		if boost > 1 {
			boost = 1
		}
		if boost < 0 {
			boost = 0
		}
	}

	decay := recency*(1-0.5*boost) + 0.5*boost
	if decay < 0.02 {
		decay = 0.02
	}
	if decay > 1 {
		decay = 1
	}
	return decay
}

// Report summarises one aging pass.
type Report struct {
	Scanned    int      `json:"scanned"`     // non-retired memories considered
	Decayed    int      `json:"decayed"`     // memories whose factor changed
	Retired    int      `json:"retired"`     // memories flagged retired this pass
	DryRun     bool     `json:"dry_run"`     // true when nothing was written
	Elapsed    float64  `json:"elapsed_ms"`  // wall time of the pass
	RetiredIDs []string `json:"retired_ids"` // ids flagged this pass (observability)
}

// String renders the report as a human-readable summary.
func (r *Report) String() string {
	verb := "retired"
	if r.DryRun {
		verb = "would retire"
	}
	base := fmt.Sprintf("aging pass: scanned %d, decayed %d, %s %d (%.0f ms)",
		r.Scanned, r.Decayed, verb, r.Retired, r.Elapsed)
	if r.DryRun {
		base += " [dry-run, no writes]"
	}
	if len(r.RetiredIDs) > 0 {
		base += fmt.Sprintf("; retired: %v", r.RetiredIDs)
	}
	return base
}

// Run executes the aging pass. In dry-run mode no writes happen; the
// report still counts what would change. Retirement is a flag
// (retired_at), never a delete: rows and their history survive, and
// UnretireMemory can restore a fact.
func Run(database *db.DB, cfg Config, now time.Time, dryRun bool) (*Report, error) {
	report := &Report{DryRun: dryRun}
	start := time.Now()
	defer func() { report.Elapsed = float64(time.Since(start).Microseconds()) / 1000.0 }()

	if !cfg.Enabled {
		return report, nil
	}

	candidates, err := database.AgingCandidates()
	if err != nil {
		return nil, fmt.Errorf("aging: load candidates: %w", err)
	}
	report.Scanned = len(candidates)

	for _, m := range candidates {
		if m.RetiredAt != nil {
			continue // safety net; AgingCandidates already excludes these
		}
		factor := DecayFactor(cfg, m.CreatedAt, m.LastAccess, m.AccessCount, now)
		retire := factor < cfg.RetireBelow

		if retire {
			if m.RetiredAt != nil {
				continue
			}
			report.Retired++
			report.RetiredIDs = append(report.RetiredIDs, m.ID)
			if dryRun {
				continue
			}
			if err := database.RetireMemory(m.ID, now); err != nil {
				return nil, fmt.Errorf("aging: retire %s: %w", m.ID, err)
			}
			continue
		}

		if math.Abs(factor-m.DecayFactor) > 1e-9 {
			report.Decayed++
			if dryRun {
				continue
			}
			if err := database.SetDecayFactor(m.ID, factor); err != nil {
				return nil, fmt.Errorf("aging: decay %s: %w", m.ID, err)
			}
		}
	}
	return report, nil
}
