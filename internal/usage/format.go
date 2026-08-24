package usage

import "strconv"

func strPtr(s string) *string { return &s }

// formatAmount formats a decimal amount as the shortest string
// representation that round-trips — avoids float64 noise (e.g. "42.75",
// never "42.750000000000004") without a full decimal-arithmetic type. The
// Swift original wraps the same float64-precision arithmetic in Decimal
// only at the boundary, so this matches its actual behavior, not just its
// declared type.
func formatAmount(v float64) string {
	return strconv.FormatFloat(v, 'f', -1, 64)
}

// parseOptionalFloat parses s as a float64; ok is false when s is nil or
// unparseable.
func parseOptionalFloat(s *string) (v float64, ok bool) {
	if s == nil {
		return 0, false
	}
	v, err := strconv.ParseFloat(*s, 64)
	if err != nil {
		return 0, false
	}
	return v, true
}
