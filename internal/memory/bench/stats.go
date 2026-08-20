package bench

import (
	"fmt"
	"math"
	"math/rand"
	"sort"
)

// This file implements the inferential statistics for the benchmark
// harness (issue #490): seeded bootstrap confidence intervals, a
// Mann-Whitney U test, Cliff's delta, and Holm-Bonferroni correction.
// Pure Go, no external dependencies; every function is deterministic for
// a fixed seed so CI comparisons are reproducible.

// defaultBootstrapIterations is the resampling count used by the
// convenience wrapper; 2000 resamples give stable percentile bounds.
const defaultBootstrapIterations = 2000

// BootstrapCI returns a percentile bootstrap confidence interval for the
// population mean of samples. rng is injected so callers can seed it for
// deterministic output. Returns an error when there are fewer than 2
// samples or the requested alpha is outside (0, 1).
func BootstrapCI(samples []float64, rng *rand.Rand, iterations int, alpha float64) (lo, hi float64, err error) {
	if len(samples) < 2 {
		return 0, 0, fmt.Errorf("bootstrap needs at least 2 samples, got %d", len(samples))
	}
	if alpha <= 0 || alpha >= 1 {
		return 0, 0, fmt.Errorf("bootstrap alpha must be in (0,1), got %v", alpha)
	}
	if iterations <= 0 {
		iterations = defaultBootstrapIterations
	}

	means := make([]float64, iterations)
	for i := 0; i < iterations; i++ {
		var sum float64
		for j := 0; j < len(samples); j++ {
			sum += samples[rng.Intn(len(samples))]
		}
		means[i] = sum / float64(len(samples))
	}
	sort.Float64s(means)

	loIdx := int(math.Floor(alpha / 2 * float64(iterations)))
	hiIdx := int(math.Ceil((1 - alpha/2) * float64(iterations)))
	if hiIdx >= iterations {
		hiIdx = iterations - 1
	}
	return means[loIdx], means[hiIdx], nil
}

// SeededBootstrapCI is BootstrapCI with a deterministic RNG seeded from
// seed, so repeated calls on the same inputs produce identical bounds.
func SeededBootstrapCI(samples []float64, seed int64, iterations int, alpha float64) (lo, hi float64, err error) {
	return BootstrapCI(samples, rand.New(rand.NewSource(seed)), iterations, alpha)
}

// rankAssignments returns tied ranks for the pooled samples, plus the
// pooled length. Used by MannWhitneyU and CliffsDelta.
func rankAssignments(a, b []float64) (ranksA []float64, n1, n2 int) {
	type item struct {
		val   float64
		fromA bool
	}
	pooled := make([]item, 0, len(a)+len(b))
	for _, v := range a {
		pooled = append(pooled, item{val: v, fromA: true})
	}
	for _, v := range b {
		pooled = append(pooled, item{val: v})
	}
	sort.Slice(pooled, func(i, j int) bool { return pooled[i].val < pooled[j].val })

	ranks := make([]float64, len(pooled))
	for i := 0; i < len(pooled); {
		j := i
		for j+1 < len(pooled) && pooled[j+1].val == pooled[i].val {
			j++
		}
		avgRank := float64(i+j+2) / 2
		for k := i; k <= j; k++ {
			ranks[k] = avgRank
		}
		i = j + 1
	}

	ranksA = make([]float64, 0, len(a))
	for i := range pooled {
		if pooled[i].fromA {
			ranksA = append(ranksA, ranks[i])
		}
	}
	return ranksA, len(a), len(b)
}

// MannWhitneyU performs a two-sided Mann-Whitney U test on independent
// samples a and b using the normal approximation with tie correction.
// It returns the U statistic (for group a), the z score, and the
// two-sided p value. Returns an error when either group has fewer than 2
// observations or the pooled variance is degenerate.
func MannWhitneyU(a, b []float64) (u, z, p float64, err error) {
	if len(a) < 2 || len(b) < 2 {
		return 0, 0, 0, fmt.Errorf("Mann-Whitney U needs at least 2 samples per group, got %d and %d", len(a), len(b))
	}
	ranksA, n1, n2 := rankAssignments(a, b)
	n := float64(n1 + n2)

	var sumRanksA float64
	for _, r := range ranksA {
		sumRanksA += r
	}
	u = sumRanksA - float64(n1*(n1+1))/2

	// Standard error with tie correction.
	// Tie groups: count frequencies of tied values in the pooled sample.
	tieCounts := tieGroupCounts(a, b)
	tieCorrection := 0.0
	for _, c := range tieCounts {
		tieCorrection += float64(c*c*c - c)
	}
	meanU := float64(n1*n2) / 2
	varVar := float64(n1*n2) / 12 * ((n + 1) - tieCorrection/(n*(n-1)))
	if varVar <= 0 {
		return 0, 0, 0, fmt.Errorf("Mann-Whitney U variance degenerate (all values tied)")
	}
	z = (u - meanU) / math.Sqrt(varVar)
	p = 2 * (1 - normalCDF(math.Abs(z)))
	return u, z, p, nil
}

// tieGroupCounts counts how many values appear more than once across the
// pooled samples, keyed by the rounded value. Floats from benchmark
// metrics are compared with a small epsilon via their rank grouping; here
// exact equality is used because samples are rational metric values
// (0, 0.5, 1, ...) or derived floats that are identical when tied.
func tieGroupCounts(a, b []float64) map[float64]int {
	counts := make(map[float64]int, len(a)+len(b))
	for _, v := range append(append([]float64{}, a...), b...) {
		counts[v]++
	}
	out := make(map[float64]int)
	for v, c := range counts {
		if c > 1 {
			out[v] = c
		}
	}
	return out
}

// normalCDF is the standard normal cumulative distribution function via
// the Abramowitz-Stegun approximation (absolute error < 7.5e-8).
func normalCDF(x float64) float64 {
	if x < 0 {
		return 1 - normalCDF(-x)
	}
	t := 1 / (1 + 0.2316419*x)
	d := 0.3989422804014327 * math.Exp(-x*x/2)
	return 1 - d*(0.319381530*t-0.356563782*t*t+1.781477937*t*t*t-1.821255978*t*t*t*t+1.330274429*t*t*t*t*t)
}

// CliffsDelta returns Cliff's dominance measure of effect size between
// samples a and b: the probability that a random a observation exceeds a
// random b observation, minus the reverse. 0 means no separation, 1
// means every a exceeds every b, -1 the opposite.
func CliffsDelta(a, b []float64) float64 {
	if len(a) == 0 || len(b) == 0 {
		return 0
	}
	// Rank-based shortcut: d = (2*sumRanksA - n1*(n1+1)) / (n1*n2) - 1,
	// computed from the pooled ranks for O((n1+n2) log(n1+n2)) cost.
	ranksA, n1, n2 := rankAssignments(a, b)
	var sumRanksA float64
	for _, r := range ranksA {
		sumRanksA += r
	}
	u := sumRanksA - float64(n1*(n1+1))/2
	return 2*u/float64(n1*n2) - 1
}

// HolmBonferroni applies the Holm-Bonferroni correction to a slice of
// p values, returning the adjusted p values (min with 1). Input order is
// preserved.
func HolmBonferroni(pvals []float64) []float64 {
	type idxVal struct {
		idx int
		p   float64
	}
	sorted := make([]idxVal, len(pvals))
	for i, p := range pvals {
		sorted[i] = idxVal{idx: i, p: p}
	}
	sort.Slice(sorted, func(i, j int) bool { return sorted[i].p < sorted[j].p })

	adjusted := make([]float64, len(pvals))
	m := len(pvals)
	prev := 0.0
	for i, sv := range sorted {
		adj := (float64(m) - float64(i)) * sv.p
		if adj < prev {
			adj = prev
		}
		if adj > 1 {
			adj = 1
		}
		prev = adj
		adjusted[sv.idx] = adj
	}
	return adjusted
}
