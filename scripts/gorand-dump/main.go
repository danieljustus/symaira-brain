// Command gorand-dump prints reference values for the Rust port of Go's
// math/rand so the native LSH projections can be pinned against the toolchain.
//
// Usage: go run ./scripts/gorand-dump
package main

import (
	"crypto/sha256"
	"encoding/binary"
	"fmt"
	"math"
	"math/rand"

	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/extractor"
)

func main() {
	source := rand.New(rand.NewSource(42))

	// Reference normals: the stream the LSH projections are drawn from.
	normals := make([]float64, 64)
	for index := range normals {
		normals[index] = source.NormFloat64()
	}
	for _, value := range normals {
		fmt.Printf("%x\n", math.Float64bits(value))
	}

	// Reference projections: 16 rows of 768 float32 normals, hashed per row
	// and as a whole so one comparison catches any drift.
	projections := make([][]float32, 16)
	// The shipped projection table is initialised from its own generator, so
	// the reference stream must not share the one used for the normals.
	projectionSource := rand.New(rand.NewSource(42))
	for row := range projections {
		projections[row] = make([]float32, 768)
		for column := range projections[row] {
			projections[row][column] = float32(projectionSource.NormFloat64())
		}
		fmt.Printf("row %d %x\n", row, rowDigest(projections[row]))
	}
	fmt.Printf("all %x\n", allDigest(projections))

	// Reference LSH hashes for the same pipeline the store uses on write:
	// ComputeLSH(GenerateLocalHashVector(text)).
	for _, text := range lshSamples {
		hash, err := db.ComputeLSH(extractor.GenerateLocalHashVector(text, db.EmbeddingDim))
		if err != nil {
			panic(err)
		}
		fmt.Printf("lsh %d\n", hash)
	}
}

// lshSamples must stay in the order the Rust pin in
// rust/symbrain-memory/src/lsh.rs lists them.
var lshSamples = []string{
	"alpha memory content",
	"beta note about rust parity",
	"Kosinus-Ranking für Suche",
}

func rowDigest(row []float32) [32]byte {
	hasher := sha256.New()
	for _, value := range row {
		var buffer [4]byte
		binary.LittleEndian.PutUint32(buffer[:], math.Float32bits(value))
		hasher.Write(buffer[:])
	}
	return [32]byte(hasher.Sum(nil))
}

func allDigest(rows [][]float32) [32]byte {
	hasher := sha256.New()
	for _, row := range rows {
		for _, value := range row {
			var buffer [4]byte
			binary.LittleEndian.PutUint32(buffer[:], math.Float32bits(value))
			hasher.Write(buffer[:])
		}
	}
	return [32]byte(hasher.Sum(nil))
}
