package db

import "fmt"

// SpaceIdentity identifies the embedding space a vector lives in.
// Two vectors may only be scored against each other when their space
// identities match: same model, same quantization level, same dimension.
// An empty quantization string denotes the legacy unquantized space.
type SpaceIdentity struct {
	Model        string
	Quantization string
	Dim          int
}

// Matches reports whether two identities describe the same embedding space.
func (s SpaceIdentity) Matches(o SpaceIdentity) bool {
	return s.Model == o.Model && s.Quantization == o.Quantization && s.Dim == o.Dim
}

// String renders the identity for diagnostics and error messages.
func (s SpaceIdentity) String() string {
	q := s.Quantization
	if q == "" {
		q = "unquantized"
	}
	return fmt.Sprintf("%s/%s/dim%d", s.Model, q, s.Dim)
}
