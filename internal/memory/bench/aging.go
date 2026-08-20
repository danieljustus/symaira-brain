package bench

import (
	"fmt"
	"math"
	"math/rand"
	"sort"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/aging"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

// AgingReport measures whether explicit aging (#491) improves retrieval
// ranking on the exact failure mode the issue describes: "a fact stated
// once eighteen months ago competes with a fact confirmed yesterday".
//
// The assessment is hermetic: a synthetic store is scored with the real
// composite scorer — once without the aging decay multiplier (status quo)
// and once with it — and the ranking of the recently-confirmed fact is
// compared. This is the "what does better recall mean" measurement the
// aging policy needs before its curve is trusted.
type AgingReport struct {
	StoreSize         int     `json:"store_size"`
	StalePct          float64 `json:"stale_pct"` // share of the store that is stale and unused
	Queries           int     `json:"queries"`
	MRRNoDecay        float64 `json:"mrr_no_decay"`            // mean reciprocal rank of the confirmed fact, no aging
	MRRDecay          float64 `json:"mrr_decay"`               // same, with the aging decay multiplier
	MRRDelta          float64 `json:"mrr_delta"`               // decay - no_decay (positive = aging helps)
	StaleCrowdNoDecay float64 `json:"stale_crowding_no_decay"` // avg stale facts in top-5, no aging
	StaleCrowdDecay   float64 `json:"stale_crowding_decay"`    // same, with aging
	StaleCrowdDelta   float64 `json:"stale_crowding_delta"`    // no_decay - decay (positive = aging clears noise)
	Description       string  `json:"description"`
}

// agingScenarioFact is one synthetic fact in the assessment store.
type agingScenarioFact struct {
	id         string
	relevance  float64 // ground-truth relevance for the query axis (0..1)
	createdAt  time.Time
	lastAccess *time.Time
	access     int64
	importance float64
	stale      bool // never re-accessed since creation
}

// RunAgingAssessment builds a synthetic store in which long-unused,
// stale-but-relevant facts compete with a recently re-confirmed fact (the
// query's answer) and fresh filler facts. It measures whether applying the
// aging decay multiplier restores ranking precision. The seed makes the
// scenario deterministic for CI.
func RunAgingAssessment(seed int64) (AgingReport, error) {
	rng := rand.New(rand.NewSource(seed))
	now := time.Now().UTC()

	const storeSize = 240
	const queries = 40
	const stalePct = 0.6
	const staleAgeDays = 500 // > 1 year: decayed to ~0.05 by the default curve
	const confirmAgoDays = 3 // the confirmed fact was re-accessed recently

	weights := db.DefaultRankingWeights()
	cfg := aging.DefaultConfig()

	facts := make([]agingScenarioFact, 0, storeSize)
	// The confirmed facts: old, high-relevance, but re-accessed recently —
	// exactly the "fact confirmed yesterday" case aging must protect.
	var confirmedIDs []int
	for i := 0; i < 8; i++ {
		f := agingScenarioFact{
			id:         fmt.Sprintf("confirmed-%02d", i),
			relevance:  0.85 + rng.Float64()*0.10,
			createdAt:  now.AddDate(0, 0, -staleAgeDays),
			access:     5 + int64(rng.Intn(6)),
			importance: 0.5,
		}
		last := now.AddDate(0, 0, -int(rng.Intn(confirmAgoDays+1)))
		f.lastAccess = &last
		facts = append(facts, f)
		confirmedIDs = append(confirmedIDs, len(facts)-1)
	}
	for i := 0; i < storeSize-8; i++ {
		f := agingScenarioFact{
			id:         fmt.Sprintf("fact-%03d", i),
			importance: 0.5,
		}
		if rng.Float64() < stalePct {
			// Stale and unused: stated once long ago, never re-accessed.
			f.relevance = 0.80 + rng.Float64()*0.15
			f.createdAt = now.AddDate(0, 0, -staleAgeDays)
			last := now.AddDate(0, 0, -staleAgeDays+int(rng.Intn(30)))
			f.lastAccess = &last
			f.access = int64(rng.Intn(2))
			f.stale = true
		} else {
			// Fresh filler: recent but low relevance — present in the store,
			// but not what the query is about.
			f.relevance = 0.30 + rng.Float64()*0.20
			f.createdAt = now.AddDate(0, 0, -int(rng.Intn(20)))
			last := now.AddDate(0, 0, -int(rng.Intn(10)))
			f.lastAccess = &last
			f.access = int64(rng.Intn(11))
		}
		facts = append(facts, f)
	}
	if len(confirmedIDs) == 0 {
		return AgingReport{}, fmt.Errorf("aging assessment: no confirmed facts generated")
	}

	score := func(f agingScenarioFact, withDecay bool) float64 {
		composite := db.CompositeScore(float32(f.relevance), f.createdAt, f.importance, f.access, f.lastAccess, nil, weights)
		if !withDecay {
			return composite
		}
		return composite * aging.DecayFactor(cfg, f.createdAt, f.lastAccess, f.access, now)
	}

	var mrrNoDecay, mrrDecay, crowdNoDecay, crowdDecay float64
	for q := 0; q < queries; q++ {
		target := confirmedIDs[q%len(confirmedIDs)]

		rank := func(withDecay bool) (mrr float64, staleInTop5 int) {
			order := make([]int, storeSize)
			for i := range order {
				order[i] = i
			}
			sort.Slice(order, func(a, b int) bool {
				sa, sb := score(facts[order[a]], withDecay), score(facts[order[b]], withDecay)
				if sa == sb {
					return facts[order[a]].id < facts[order[b]].id
				}
				return sa > sb
			})
			for rankIdx, idx := range order {
				if idx == target {
					mrr = 1.0 / float64(rankIdx+1)
					break
				}
			}
			for _, idx := range order[:5] {
				if facts[idx].stale {
					staleInTop5++
				}
			}
			return mrr, staleInTop5
		}

		mrr1, s1 := rank(false)
		mrr2, s2 := rank(true)
		mrrNoDecay += mrr1
		crowdNoDecay += float64(s1)
		mrrDecay += mrr2
		crowdDecay += float64(s2)
	}

	report := AgingReport{
		StoreSize:         storeSize,
		StalePct:          stalePct * 100,
		Queries:           queries,
		MRRNoDecay:        round4(mrrNoDecay / queries),
		MRRDecay:          round4(mrrDecay / queries),
		MRRDelta:          round4((mrrDecay - mrrNoDecay) / queries),
		StaleCrowdNoDecay: round4(crowdNoDecay / queries),
		StaleCrowdDecay:   round4(crowdDecay / queries),
		StaleCrowdDelta:   round4((crowdNoDecay - crowdDecay) / queries),
		Description: "Hermetic synthetic-store comparison on the #491 failure mode: stale facts " +
			"stated once long ago compete with a recently re-confirmed fact. MRR tracks the " +
			"confirmed fact's rank; stale crowding is the average number of stale facts in the " +
			"top-5. Positive MRR and crowding deltas mean aging restores precision.",
	}
	return report, nil
}

func round4(v float64) float64 {
	return math.Round(v*10000) / 10000
}
