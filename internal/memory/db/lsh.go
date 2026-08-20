package db

import (
	"fmt"
	"math/rand"
	"sync"
)

const (
	// LSHBits controls the number of hash bits (and thus bucket count = 2^LSHBits).
	// 16 bits → 65,536 buckets for good recall on datasets up to ~100K memories.
	LSHBits = 16
	// EmbeddingDim is the expected vector dimensionality.
	EmbeddingDim = 768
)

var (
	lshProjectionsOnce sync.Once
	lshProjections     [][]float32
)

func getLSHProjections() [][]float32 {
	lshProjectionsOnce.Do(func() {
		rng := rand.New(rand.NewSource(42))
		lshProjections = make([][]float32, LSHBits)
		for i := range lshProjections {
			lshProjections[i] = make([]float32, EmbeddingDim)
			for j := range lshProjections[i] {
				lshProjections[i][j] = float32(rng.NormFloat64())
			}
		}
	})
	return lshProjections
}

// ComputeLSH computes the LSH hash bits for a vector.
// Returns an integer where each bit represents the sign of the dot product
// with one of the fixed random projection vectors.
// A vector whose dimension differs from EmbeddingDim is a space mismatch and
// returns an error instead of being silently truncated.
func ComputeLSH(vec []float32) (int, error) {
	if len(vec) == 0 {
		return 0, nil
	}
	if len(vec) != EmbeddingDim {
		return 0, fmt.Errorf("lsh: dimension mismatch: got %d, expected %d", len(vec), EmbeddingDim)
	}
	projections := getLSHProjections()
	var hash int
	for i, proj := range projections {
		var dot float64
		for j := range EmbeddingDim {
			dot += float64(vec[j] * proj[j])
		}
		if dot >= 0 {
			hash |= 1 << i
		}
	}
	return hash, nil
}

// LSHNeighbors returns all LSH hashes within the given Hamming distance of base.
// The result always includes base itself (distance 0).
func LSHNeighbors(base int, maxDistance int) []int {
	if maxDistance <= 0 {
		return []int{base}
	}
	var neighbors []int
	var dfs func(idx int, dist int, current int)
	dfs = func(idx int, dist int, current int) {
		if idx == LSHBits {
			neighbors = append(neighbors, current)
			return
		}
		// Keep bit as-is
		dfs(idx+1, dist, current)
		// Flip bit if we still have distance budget
		if dist < maxDistance {
			mask := 1 << idx
			dfs(idx+1, dist+1, current^mask)
		}
	}
	dfs(0, 0, base)
	return neighbors
}
