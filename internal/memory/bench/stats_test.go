package bench

import (
	"math"
	"math/rand"
	"sort"
	"testing"
)

func TestBootstrapCI_ContainsMean(t *testing.T) {
	rng := rand.New(rand.NewSource(42))
	samples := make([]float64, 100)
	for i := range samples {
		samples[i] = 0.5 + float64(i%7)*0.01
	}
	lo, hi, err := BootstrapCI(samples, rng, 2000, 0.05)
	if err != nil {
		t.Fatalf("BootstrapCI failed: %v", err)
	}
	var sum float64
	for _, s := range samples {
		sum += s
	}
	mean := sum / float64(len(samples))
	if lo > mean || hi < mean {
		t.Errorf("CI [%f, %f] does not contain the mean %f", lo, hi, mean)
	}
	if lo >= hi {
		t.Errorf("degenerate CI: [%f, %f]", lo, hi)
	}
}

func TestSeededBootstrapCI_Deterministic(t *testing.T) {
	samples := []float64{0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9}
	lo1, hi1, err := SeededBootstrapCI(samples, 12345, 1000, 0.05)
	if err != nil {
		t.Fatal(err)
	}
	lo2, hi2, err := SeededBootstrapCI(samples, 12345, 1000, 0.05)
	if err != nil {
		t.Fatal(err)
	}
	if lo1 != lo2 || hi1 != hi2 {
		t.Errorf("seeded bootstrap not deterministic: [%f,%f] vs [%f,%f]", lo1, hi1, lo2, hi2)
	}
}

func TestBootstrapCI_Errors(t *testing.T) {
	if _, _, err := BootstrapCI([]float64{1.0}, rand.New(rand.NewSource(1)), 100, 0.05); err == nil {
		t.Error("expected error for a single sample")
	}
	if _, _, err := BootstrapCI([]float64{1.0, 2.0}, rand.New(rand.NewSource(1)), 100, 0); err == nil {
		t.Error("expected error for alpha=0")
	}
	if _, _, err := BootstrapCI([]float64{1.0, 2.0}, rand.New(rand.NewSource(1)), 100, 1); err == nil {
		t.Error("expected error for alpha=1")
	}
}

func TestMannWhitneyU_EqualGroups(t *testing.T) {
	a := []float64{0.5, 0.6, 0.5, 0.7, 0.55, 0.65, 0.5, 0.6}
	b := []float64{0.55, 0.5, 0.6, 0.5, 0.7, 0.5, 0.6, 0.55}
	_, _, p, err := MannWhitneyU(a, b)
	if err != nil {
		t.Fatalf("MannWhitneyU failed: %v", err)
	}
	if p < 0.01 {
		t.Errorf("equal groups must not be significant, p=%f", p)
	}
}

func TestMannWhitneyU_ShiftedGroups(t *testing.T) {
	a := []float64{0.9, 0.95, 0.85, 0.92, 0.88, 0.94, 0.9, 0.91}
	b := []float64{0.1, 0.2, 0.15, 0.3, 0.05, 0.25, 0.2, 0.1}
	_, _, p, err := MannWhitneyU(a, b)
	if err != nil {
		t.Fatalf("MannWhitneyU failed: %v", err)
	}
	if p > 0.05 {
		t.Errorf("shifted groups must be significant, p=%f", p)
	}
}

func TestMannWhitneyU_Deterministic(t *testing.T) {
	a := []float64{0.8, 0.7, 0.6, 0.9}
	b := []float64{0.2, 0.3, 0.4, 0.1}
	_, z1, p1, err := MannWhitneyU(a, b)
	if err != nil {
		t.Fatal(err)
	}
	_, z2, p2, err := MannWhitneyU(a, b)
	if err != nil {
		t.Fatal(err)
	}
	if z1 != z2 || p1 != p2 {
		t.Errorf("Mann-Whitney U not deterministic")
	}
}

func TestCliffsDelta(t *testing.T) {
	a := []float64{0.9, 0.95, 0.85, 0.92}
	b := []float64{0.1, 0.2, 0.15, 0.3}
	d := CliffsDelta(a, b)
	if d < 0.5 {
		t.Errorf("expected large positive Cliff's delta, got %f", d)
	}
	if d := CliffsDelta(a, a); d != 0 {
		t.Errorf("identical groups must have delta 0, got %f", d)
	}
	if d := CliffsDelta(b, a); d > -0.5 {
		t.Errorf("reversed groups must have negative delta, got %f", d)
	}
	if d := CliffsDelta(nil, a); d != 0 {
		t.Errorf("empty group must yield delta 0, got %f", d)
	}
}

func TestHolmBonferroni(t *testing.T) {
	pvals := []float64{0.01, 0.2, 0.05, 0.5}
	adjusted := HolmBonferroni(pvals)
	if adjusted[0] != 0.04 {
		t.Errorf("smallest p must scale by m, got %f", adjusted[0])
	}
	// Monotonicity holds in sorted order, not input order.
	sorted := append([]float64{}, adjusted...)
	sort.Float64s(sorted)
	for i := 1; i < len(sorted); i++ {
		if sorted[i] < sorted[i-1] {
			t.Errorf("Holm output must be monotonic in sorted order, got %v", adjusted)
		}
	}
	for _, a := range adjusted {
		if a > 1 {
			t.Errorf("adjusted p must cap at 1, got %f", a)
		}
	}
	// Input order is preserved: the second input's adjusted value is
	// 2*m*p=0.4, the third's is 3*m*p=0.15 (within float tolerance).
	if math.Abs(adjusted[1]-0.4) > 1e-9 || math.Abs(adjusted[2]-0.15) > 1e-9 {
		t.Errorf("expected input-order preservation [0.04 0.4 0.15 0.5], got %v", adjusted)
	}
}

func TestNormalCDF(t *testing.T) {
	if math.Abs(normalCDF(0)-0.5) > 1e-6 {
		t.Errorf("normalCDF(0) = %f", normalCDF(0))
	}
	if math.Abs(normalCDF(1.96)-0.975) > 1e-3 {
		t.Errorf("normalCDF(1.96) = %f", normalCDF(1.96))
	}
	if math.Abs(normalCDF(-1.96)-(1-0.975)) > 1e-3 {
		t.Errorf("normalCDF(-1.96) = %f", normalCDF(-1.96))
	}
}
