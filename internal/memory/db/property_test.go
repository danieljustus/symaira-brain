package db

import (
	"encoding/json"
	"testing"
	"time"

	"pgregory.net/rapid"
)

// ---------------------------------------------------------------------------
// BinarizeVector determinism
// ---------------------------------------------------------------------------

func TestProperty_BinarizeVectorDeterministic(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		vec := rapid.SliceOfN(rapid.Float32().Filter(func(f float32) bool {
			return !isNaN32(f)
		}), 0, EmbeddingBits).Draw(t, "vec")

		first := BinarizeVector(vec)
		second := BinarizeVector(vec)

		if len(first) != EmbeddingBytes {
			t.Fatalf("expected %d bytes, got %d", EmbeddingBytes, len(first))
		}
		if len(second) != EmbeddingBytes {
			t.Fatalf("expected %d bytes, got %d", EmbeddingBytes, len(second))
		}

		for i := range first {
			if first[i] != second[i] {
				t.Fatalf("non-deterministic binarization at byte %d: %08b vs %08b", i, first[i], second[i])
			}
		}
	})
}

func TestProperty_BinarizeVectorSameInputSameOutput(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		vec := rapid.SliceOfN(rapid.Float32().Filter(func(f float32) bool {
			return !isNaN32(f)
		}), 0, EmbeddingBits).Draw(t, "vec")

		result := BinarizeVector(vec)
		expected := BinarizeVector(vec)
		for i := range result {
			if result[i] != expected[i] {
				t.Fatalf("determinism violation at byte %d", i)
			}
		}
	})
}

// ---------------------------------------------------------------------------
// HammingDistance symmetry / d(a,a)=0
// ---------------------------------------------------------------------------

func TestProperty_HammingDistanceSelfIsZero(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		data := rapid.SliceOfN(rapid.Byte(), EmbeddingBytes, EmbeddingBytes).Draw(t, "data")

		dist := HammingDistance(data, data)
		if dist != 0 {
			t.Fatalf("HammingDistance(a, a) must be 0, got %d", dist)
		}

		zeros := make([]byte, EmbeddingBytes)
		dist = HammingDistance(zeros, zeros)
		if dist != 0 {
			t.Fatalf("HammingDistance(zeros, zeros) must be 0, got %d", dist)
		}
	})
}

func TestProperty_HammingDistanceSymmetric(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		a := rapid.SliceOfN(rapid.Byte(), EmbeddingBytes, EmbeddingBytes).Draw(t, "a")
		b := rapid.SliceOfN(rapid.Byte(), EmbeddingBytes, EmbeddingBytes).Draw(t, "b")

		distAB := HammingDistance(a, b)
		distBA := HammingDistance(b, a)

		if distAB != distBA {
			t.Fatalf("HammingDistance must be symmetric: d(a,b)=%d, d(b,a)=%d", distAB, distBA)
		}
	})
}

func TestProperty_HammingDistanceTriangleInequality(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		a := rapid.SliceOfN(rapid.Byte(), EmbeddingBytes, EmbeddingBytes).Draw(t, "a")
		b := rapid.SliceOfN(rapid.Byte(), EmbeddingBytes, EmbeddingBytes).Draw(t, "b")
		c := rapid.SliceOfN(rapid.Byte(), EmbeddingBytes, EmbeddingBytes).Draw(t, "c")

		ab := HammingDistance(a, b)
		bc := HammingDistance(b, c)
		ac := HammingDistance(a, c)

		if ac > ab+bc {
			t.Fatalf("Hamming triangle inequality violated: d(a,c)=%d > d(a,b)=%d + d(b,c)=%d", ac, ab, bc)
		}
	})
}

func TestProperty_HammingDistanceBounded(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		a := rapid.SliceOfN(rapid.Byte(), EmbeddingBytes, EmbeddingBytes).Draw(t, "a")
		b := rapid.SliceOfN(rapid.Byte(), EmbeddingBytes, EmbeddingBytes).Draw(t, "b")

		dist := HammingDistance(a, b)
		if dist < 0 || dist > EmbeddingBits {
			t.Fatalf("HammingDistance out of bounds [0, %d]: got %d", EmbeddingBits, dist)
		}
	})
}

// ---------------------------------------------------------------------------
// Memory (oplog) JSON encode → decode round-trip
// ---------------------------------------------------------------------------

func TestProperty_MemoryJSONRoundTrip(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		original := genMemory(t)

		data, err := json.Marshal(original)
		if err != nil {
			t.Fatalf("json.Marshal failed: %v", err)
		}

		var decoded Memory
		if err := json.Unmarshal(data, &decoded); err != nil {
			t.Fatalf("json.Unmarshal failed: %v", err)
		}

		if decoded.ID != original.ID {
			t.Fatalf("ID mismatch: %q vs %q", decoded.ID, original.ID)
		}
		if decoded.Content != original.Content {
			t.Fatalf("Content mismatch: %q vs %q", decoded.Content, original.Content)
		}
		if decoded.Scope != original.Scope {
			t.Fatalf("Scope mismatch: %q vs %q", decoded.Scope, original.Scope)
		}
		if len(decoded.Embedding) != len(original.Embedding) {
			t.Fatalf("Embedding length mismatch: %d vs %d", len(decoded.Embedding), len(original.Embedding))
		}
		for i := range original.Embedding {
			if decoded.Embedding[i] != original.Embedding[i] {
				t.Fatalf("Embedding[%d] mismatch: %f vs %f", i, decoded.Embedding[i], original.Embedding[i])
			}
		}
		if decoded.EmbeddingSource != original.EmbeddingSource {
			t.Fatalf("EmbeddingSource mismatch: %q vs %q", decoded.EmbeddingSource, original.EmbeddingSource)
		}
		if decoded.EmbeddingModel != original.EmbeddingModel {
			t.Fatalf("EmbeddingModel mismatch: %q vs %q", decoded.EmbeddingModel, original.EmbeddingModel)
		}
		if decoded.ContentHash != original.ContentHash {
			t.Fatalf("ContentHash mismatch: %q vs %q", decoded.ContentHash, original.ContentHash)
		}
		if !decoded.CreatedAt.Equal(original.CreatedAt) {
			t.Fatalf("CreatedAt mismatch: %v vs %v", decoded.CreatedAt, original.CreatedAt)
		}
		if !decoded.UpdatedAt.Equal(original.UpdatedAt) {
			t.Fatalf("UpdatedAt mismatch: %v vs %v", decoded.UpdatedAt, original.UpdatedAt)
		}
		if decoded.ConsolidationStatus != original.ConsolidationStatus {
			t.Fatalf("ConsolidationStatus mismatch: %q vs %q", decoded.ConsolidationStatus, original.ConsolidationStatus)
		}
		if decoded.Importance != original.Importance {
			t.Fatalf("Importance mismatch: %f vs %f", decoded.Importance, original.Importance)
		}
		if decoded.Tier != original.Tier {
			t.Fatalf("Tier mismatch: %q vs %q", decoded.Tier, original.Tier)
		}
	})
}

// genMemory generates a random Memory using rapid generators for the JSON round-trip test.
func genMemory(t *rapid.T) *Memory {
	scope := rapid.Just("global").Draw(t, "scope")
	tier := rapid.Just("long_term").Draw(t, "tier")

	return &Memory{
		ID:                  rapid.StringMatching(`[a-f0-9\-]{20,40}`).Draw(t, "id"),
		Content:             rapid.String().Draw(t, "content"),
		Scope:               scope,
		Metadata:            map[string]string{},
		Embedding:           rapid.SliceOfN(rapid.Float32(), 0, 4).Draw(t, "embedding"),
		EmbeddingSource:     rapid.String().Draw(t, "embSource"),
		EmbeddingModel:      rapid.String().Draw(t, "embModel"),
		ContentHash:         rapid.String().Draw(t, "contentHash"),
		CreatedAt:           genTime(t, "createdAt"),
		UpdatedAt:           genTime(t, "updatedAt"),
		CreatedBy:           rapid.String().Draw(t, "createdBy"),
		UpdatedBy:           rapid.String().Draw(t, "updatedBy"),
		CreatedSession:      rapid.String().Draw(t, "createdSession"),
		UpdatedSession:      rapid.String().Draw(t, "updatedSession"),
		ConsolidationStatus: rapid.String().Draw(t, "consolidationStatus"),
		ConsolidatedIntoID:  rapid.String().Draw(t, "consolidatedIntoID"),
		Importance:          rapid.Float64Range(0, 1).Draw(t, "importance"),
		Tier:                tier,
	}
}

func genTime(t *rapid.T, name string) time.Time {
	sec := rapid.Int64Range(0, 2000000000).Draw(t, name+".sec")
	nsec := rapid.Int64Range(0, 999999999).Draw(t, name+".nsec")
	return time.Unix(sec, nsec).UTC()
}

func isNaN32(f float32) bool {
	return f != f
}

// ---------------------------------------------------------------------------
// LSH band-hash determinism
// ---------------------------------------------------------------------------

func TestProperty_LSHDeterministic(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		vec := rapid.SliceOfN(rapid.Float32(), EmbeddingDim, EmbeddingDim).Draw(t, "vec")

		first := mustComputeLSH(vec)
		second := mustComputeLSH(vec)
		third := mustComputeLSH(vec)

		if first != second || second != third {
			t.Fatalf("LSH hash must be deterministic: got %d, %d, %d", first, second, third)
		}
	})
}

func TestProperty_LSHBitCount(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		vec := rapid.SliceOfN(rapid.Float32(), EmbeddingDim, EmbeddingDim).Draw(t, "vec")

		hash := mustComputeLSH(vec)

		maxHash := 1 << LSHBits
		if hash < 0 || hash >= maxHash {
			t.Fatalf("LSH hash %d out of range [0, %d)", hash, maxHash)
		}
	})
}

func TestProperty_LSHConsistentWithBinarization(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		vec := rapid.SliceOfN(rapid.Float32().Filter(func(f float32) bool {
			return !isNaN32(f)
		}), EmbeddingDim, EmbeddingDim).Draw(t, "vec")

		h1 := mustComputeLSH(vec)
		h2 := mustComputeLSH(vec)
		if h1 != h2 {
			t.Fatalf("LSH must be deterministic: %d vs %d", h1, h2)
		}

		if h1 < 0 || h1 >= (1<<LSHBits) {
			t.Fatalf("hash %d out of range [0, %d)", h1, 1<<LSHBits)
		}
	})
}
