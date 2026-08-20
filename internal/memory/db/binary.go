package db

import (
	"encoding/binary"
	"math/bits"
)

const (
	// EmbeddingBits is the expected bit dimensionality for binary embeddings.
	// Must match EmbeddingDim (768) — each float32 sign becomes one bit.
	EmbeddingBits = 768
	// EmbeddingBytes is the byte length of a binarized EmbeddingBits vector.
	EmbeddingBytes = EmbeddingBits / 8 // 96
)

// BinarizeVector converts a float32 embedding to a compact sign-bit binary
// vector. Each element's sign is packed into a single bit: positive → 0,
// negative or zero → 1. Bits are packed LSB-first within each uint64 word,
// producing a 96-byte (768-bit) result.
func BinarizeVector(vec []float32) []byte {
	if len(vec) == 0 {
		return make([]byte, EmbeddingBytes)
	}
	buf := make([]byte, EmbeddingBytes)
	n := len(vec)
	if n > EmbeddingBits {
		n = EmbeddingBits
	}
	// Pack bits LSB-first into uint64 words.
	for i := 0; i < n; i++ {
		if vec[i] <= 0 {
			wordIdx := i / 64
			bitIdx := uint(i % 64)
			off := wordIdx * 8
			// Read existing word, set bit, write back (little-endian).
			word := binary.LittleEndian.Uint64(buf[off:])
			word |= 1 << bitIdx
			binary.LittleEndian.PutUint64(buf[off:], word)
		}
	}
	return buf
}

// HammingDistance returns the number of differing bits between two binary
// vectors of equal length. Both must be exactly EmbeddingBytes long.
func HammingDistance(a, b []byte) int {
	if len(a) != len(b) {
		return EmbeddingBits // maximum distance for mismatched lengths
	}
	dist := 0
	for i := 0; i < len(a)-7; i += 8 {
		wa := binary.LittleEndian.Uint64(a[i:])
		wb := binary.LittleEndian.Uint64(b[i:])
		dist += bits.OnesCount64(wa ^ wb)
	}
	return dist
}

// HammingPrefilter returns the indices of the n closest candidates by Hamming
// distance to queryBin. Ties are broken by keeping the earlier index.
// If len(candidates) <= n, all indices are returned (no filtering needed).
func HammingPrefilter(queryBin []byte, candidates [][]byte, n int) []int {
	if n <= 0 || len(candidates) == 0 {
		return nil
	}
	if len(candidates) <= n {
		result := make([]int, len(candidates))
		for i := range result {
			result[i] = i
		}
		return result
	}

	type entry struct {
		idx  int
		dist int
	}

	entries := make([]entry, len(candidates))
	for i, c := range candidates {
		if c == nil {
			entries[i] = entry{idx: i, dist: EmbeddingBits} // nil → max distance
			continue
		}
		entries[i] = entry{idx: i, dist: HammingDistance(queryBin, c)}
	}

	// Partial sort: find the n smallest distances. Stable: ties keep original index order.
	// Simple insertion-based selection for small n relative to len(candidates).
	selected := make([]entry, 0, n)
	for _, e := range entries {
		if len(selected) < n {
			// Insert into sorted position.
			pos := len(selected)
			selected = append(selected, entry{})
			copy(selected[pos+1:], selected[pos:])
			selected[pos] = e
			// Bubble up to maintain sort by (dist ascending, idx ascending).
			for j := pos; j > 0; j-- {
				if selected[j].dist < selected[j-1].dist ||
					(selected[j].dist == selected[j-1].dist && selected[j].idx < selected[j-1].idx) {
					selected[j], selected[j-1] = selected[j-1], selected[j]
				} else {
					break
				}
			}
		} else if e.dist < selected[n-1].dist ||
			(e.dist == selected[n-1].dist && e.idx < selected[n-1].idx) {
			// Replace the worst and re-sort.
			selected[n-1] = e
			for j := n - 1; j > 0; j-- {
				if selected[j].dist < selected[j-1].dist ||
					(selected[j].dist == selected[j-1].dist && selected[j].idx < selected[j-1].idx) {
					selected[j], selected[j-1] = selected[j-1], selected[j]
				} else {
					break
				}
			}
		}
	}

	result := make([]int, len(selected))
	for i, e := range selected {
		result[i] = e.idx
	}
	return result
}
