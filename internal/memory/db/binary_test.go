package db

import (
	"math/rand"
	"testing"
)

func TestBinarizeVector_AllPositive(t *testing.T) {
	vec := make([]float32, EmbeddingBits)
	for i := range vec {
		vec[i] = float32(i+1) / float32(EmbeddingBits)
	}
	bin := BinarizeVector(vec)
	if len(bin) != EmbeddingBytes {
		t.Fatalf("expected %d bytes, got %d", EmbeddingBytes, len(bin))
	}
	for _, b := range bin {
		if b != 0 {
			t.Fatalf("all positive vector should produce all-zero bytes, got non-zero: %08b", b)
		}
	}
}

func TestBinarizeVector_AllNegative(t *testing.T) {
	vec := make([]float32, EmbeddingBits)
	for i := range vec {
		vec[i] = -float32(i+1) / float32(EmbeddingBits)
	}
	bin := BinarizeVector(vec)
	if len(bin) != EmbeddingBytes {
		t.Fatalf("expected %d bytes, got %d", EmbeddingBytes, len(bin))
	}
	for _, b := range bin {
		if b != 0xFF {
			t.Fatalf("all negative vector should produce all-0xFF bytes, got: %08b", b)
		}
	}
}

func TestBinarizeVector_ZeroValues(t *testing.T) {
	vec := make([]float32, EmbeddingBits)
	for i := range vec {
		vec[i] = 0
	}
	bin := BinarizeVector(vec)
	for _, b := range bin {
		if b != 0xFF {
			t.Fatalf("zero-valued vector should produce all-0xFF bytes (zero ≤ 0 → bit=1), got: %08b", b)
		}
	}
}

func TestBinarizeVector_Mixed(t *testing.T) {
	vec := make([]float32, EmbeddingBits)
	for i := range vec {
		if i%2 == 0 {
			vec[i] = 1.0
		} else {
			vec[i] = -1.0
		}
	}
	bin := BinarizeVector(vec)
	if len(bin) != EmbeddingBytes {
		t.Fatalf("expected %d bytes, got %d", EmbeddingBytes, len(bin))
	}
	for i, b := range bin {
		expected := byte(0xAA) // 10101010 — odd bits set (negative values)
		if b != expected {
			t.Fatalf("byte %d: expected %08b, got %08b", i, expected, b)
		}
	}
}

func TestBinarizeVector_Empty(t *testing.T) {
	bin := BinarizeVector(nil)
	if len(bin) != EmbeddingBytes {
		t.Fatalf("nil input should produce %d-byte zero buffer, got %d", EmbeddingBytes, len(bin))
	}
	for _, b := range bin {
		if b != 0 {
			t.Fatal("nil input should produce all-zero bytes")
		}
	}
}

func TestBinarizeVector_TooLong(t *testing.T) {
	vec := make([]float32, EmbeddingBits+100)
	for i := range vec {
		vec[i] = -1.0
	}
	bin := BinarizeVector(vec)
	if len(bin) != EmbeddingBytes {
		t.Fatalf("expected %d bytes, got %d", EmbeddingBytes, len(bin))
	}
	for _, b := range bin {
		if b != 0xFF {
			t.Fatal("only first EmbeddingBits should be used; rest truncated")
		}
	}
}

func TestBinarizeVector_ShortVector(t *testing.T) {
	vec := []float32{-1, 1, -1}
	bin := BinarizeVector(vec)
	if len(bin) != EmbeddingBytes {
		t.Fatalf("expected %d bytes, got %d", EmbeddingBytes, len(bin))
	}
	// First byte: bit 0 = 1 (negative), bit 1 = 0 (positive), bit 2 = 1 (negative) → 0b101 = 5
	if bin[0] != 5 {
		t.Fatalf("expected first byte 0b00000101, got %08b", bin[0])
	}
	// All other bytes should be 0
	for i := 1; i < len(bin); i++ {
		if bin[i] != 0 {
			t.Fatalf("byte %d should be 0 for short vector, got %08b", i, bin[i])
		}
	}
}

func TestHammingDistance_Identical(t *testing.T) {
	vec := make([]float32, EmbeddingBits)
	for i := range vec {
		vec[i] = float32(i) - 384.0
	}
	bin := BinarizeVector(vec)
	dist := HammingDistance(bin, bin)
	if dist != 0 {
		t.Fatalf("identical vectors should have distance 0, got %d", dist)
	}
}

func TestHammingDistance_CompletelyOpposite(t *testing.T) {
	pos := make([]float32, EmbeddingBits)
	neg := make([]float32, EmbeddingBits)
	for i := range pos {
		pos[i] = 1.0
		neg[i] = -1.0
	}
	binPos := BinarizeVector(pos)
	binNeg := BinarizeVector(neg)
	dist := HammingDistance(binPos, binNeg)
	if dist != EmbeddingBits {
		t.Fatalf("completely opposite vectors should have distance %d, got %d", EmbeddingBits, dist)
	}
}

func TestHammingDistance_MismatchedLengths(t *testing.T) {
	a := make([]byte, EmbeddingBytes)
	b := make([]byte, EmbeddingBytes-1)
	dist := HammingDistance(a, b)
	if dist != EmbeddingBits {
		t.Fatalf("mismatched lengths should return max distance %d, got %d", EmbeddingBits, dist)
	}
}

func TestHammingDistance_PartialDiff(t *testing.T) {
	a := make([]byte, EmbeddingBytes)
	b := make([]byte, EmbeddingBytes)
	// Set bit 0 in a only
	a[0] = 1
	dist := HammingDistance(a, b)
	if dist != 1 {
		t.Fatalf("expected distance 1, got %d", dist)
	}
}

func TestHammingPrefilter_SelectsTopN(t *testing.T) {
	query := make([]float32, EmbeddingBits)
	for i := range query {
		query[i] = 1.0
	}
	queryBin := BinarizeVector(query)

	candidates := make([][]byte, 10)
	for i := range candidates {
		vec := make([]float32, EmbeddingBits)
		for j := range vec {
			vec[j] = float32(i) // different constant per candidate
		}
		candidates[i] = BinarizeVector(vec)
	}

	result := HammingPrefilter(queryBin, candidates, 3)
	if len(result) != 3 {
		t.Fatalf("expected 3 results, got %d", len(result))
	}

	for _, idx := range result {
		if idx < 0 || idx >= len(candidates) {
			t.Fatalf("index %d out of range", idx)
		}
	}
}

func TestHammingPrefilter_AllCandidatesWhenNExceedsLen(t *testing.T) {
	candidates := make([][]byte, 5)
	for i := range candidates {
		candidates[i] = make([]byte, EmbeddingBytes)
	}
	query := make([]byte, EmbeddingBytes)

	result := HammingPrefilter(query, candidates, 100)
	if len(result) != 5 {
		t.Fatalf("expected all 5 indices when n > len, got %d", len(result))
	}
	for i, idx := range result {
		if idx != i {
			t.Fatalf("expected sequential indices, got %d at position %d", idx, i)
		}
	}
}

func TestHammingPrefilter_EmptyCandidates(t *testing.T) {
	query := make([]byte, EmbeddingBytes)
	result := HammingPrefilter(query, nil, 10)
	if result != nil {
		t.Fatalf("expected nil for empty candidates, got %v", result)
	}
}

func TestHammingPrefilter_ZeroN(t *testing.T) {
	query := make([]byte, EmbeddingBytes)
	candidates := [][]byte{make([]byte, EmbeddingBytes)}
	result := HammingPrefilter(query, candidates, 0)
	if result != nil {
		t.Fatalf("expected nil for n=0, got %v", result)
	}
}

func TestHammingPrefilter_NilCandidates(t *testing.T) {
	query := make([]byte, EmbeddingBytes)
	candidates := [][]byte{nil, make([]byte, EmbeddingBytes), nil}
	result := HammingPrefilter(query, candidates, 2)
	if len(result) != 2 {
		t.Fatalf("expected 2 results, got %d", len(result))
	}
	for _, idx := range result {
		if idx < 0 || idx >= len(candidates) {
			t.Fatalf("index %d out of range", idx)
		}
	}
}

func TestHammingPrefilter_TieBreaking(t *testing.T) {
	query := make([]byte, EmbeddingBytes)
	// All candidates have same distance (0) from query
	candidates := make([][]byte, 5)
	for i := range candidates {
		candidates[i] = make([]byte, EmbeddingBytes)
	}

	result := HammingPrefilter(query, candidates, 3)
	if len(result) != 3 {
		t.Fatalf("expected 3 results, got %d", len(result))
	}
	// Ties broken by earlier index
	for i, idx := range result {
		if idx != i {
			t.Fatalf("tie-breaking should prefer earlier index: expected %d at position %d, got %d", i, i, idx)
		}
	}
}

func BenchmarkHammingPrefilter(b *testing.B) {
	rng := rand.New(rand.NewSource(42))
	query := make([]float32, EmbeddingBits)
	for i := range query {
		query[i] = rng.Float32()*2 - 1
	}
	queryBin := BinarizeVector(query)

	const numCandidates = 10000
	candidates := make([][]byte, numCandidates)
	for i := range candidates {
		vec := make([]float32, EmbeddingBits)
		for j := range vec {
			vec[j] = rng.Float32()*2 - 1
		}
		candidates[i] = BinarizeVector(vec)
	}

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		HammingPrefilter(queryBin, candidates, 100)
	}
}

func BenchmarkBinarizeVector(b *testing.B) {
	vec := make([]float32, EmbeddingBits)
	for i := range vec {
		vec[i] = float32(i) - 384.0
	}
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		BinarizeVector(vec)
	}
}

func BenchmarkHammingDistance(b *testing.B) {
	rng := rand.New(rand.NewSource(42))
	vec := make([]float32, EmbeddingBits)
	for i := range vec {
		vec[i] = rng.Float32()*2 - 1
	}
	bin := BinarizeVector(vec)
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		HammingDistance(bin, bin)
	}
}
