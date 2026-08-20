// Package entity provides pure, DB-independent matching primitives for
// resolving entity candidates by name or alias: Unicode-aware normalization,
// scored exact/normalized comparison, and PII-shaped hint rejection.
package entity

import (
	"fmt"
	"regexp"
	"strings"
	"unicode"

	"golang.org/x/text/runes"
	"golang.org/x/text/transform"
	"golang.org/x/text/unicode/norm"
)

// MatchKind identifies which comparison produced a candidate match.
type MatchKind string

const (
	MatchExactName       MatchKind = "exact_name"
	MatchExactAlias      MatchKind = "exact_alias"
	MatchNormalizedName  MatchKind = "normalized_name"
	MatchNormalizedAlias MatchKind = "normalized_alias"
)

// Fixed scores per match kind. Exact matches always outrank normalized
// matches; name matches slightly outrank alias matches of the same strength.
const (
	scoreExactName       = 1.0
	scoreExactAlias      = 0.9
	scoreNormalizedName  = 0.75
	scoreNormalizedAlias = 0.65
)

// MaxHintLength bounds a single query or alias hint so a pathological input
// cannot blow up normalization or comparison cost.
const MaxHintLength = 500

var diacriticStripper = transform.Chain(norm.NFD, runes.Remove(runes.In(unicode.Mn)), norm.NFC)

// Normalize returns a comparison-safe form of s: Unicode NFD-decomposed with
// combining diacritical marks removed, case-folded, outer whitespace
// trimmed, and internal whitespace runs collapsed to a single space.
func Normalize(s string) string {
	folded, _, err := transform.String(diacriticStripper, s)
	if err != nil {
		folded = s
	}
	folded = strings.ToLower(folded)
	return strings.Join(strings.Fields(folded), " ")
}

var (
	emailRe = regexp.MustCompile(`(?i)^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$`)
	phoneRe = regexp.MustCompile(`^\+?[0-9()\-.\s]+$`)
)

// IsPII reports whether s looks like an email address or a phone number and
// should therefore be rejected as an alias comparison hint by default, so
// contact identifiers never become implicit Memory data.
func IsPII(s string) bool {
	s = strings.TrimSpace(s)
	if s == "" {
		return false
	}
	if emailRe.MatchString(s) {
		return true
	}
	if !phoneRe.MatchString(s) {
		return false
	}
	digits := 0
	for _, r := range s {
		if unicode.IsDigit(r) {
			digits++
		}
	}
	return digits >= 6
}

// BestMatch compares every hint (the primary query plus any extra alias
// hints, already filtered for PII by the caller) against a single entity's
// name and aliases. It returns the strongest match found — exact matches
// outrank normalized matches, and among equal-strength matches the first
// hint in input order wins, keeping the result deterministic. ok is false
// when no hint matched.
func BestMatch(hints []string, name string, aliases []string) (kind MatchKind, reason string, score float64, ok bool) {
	normName := Normalize(name)

	consider := func(k MatchKind, s float64, hint, target string) {
		if ok && s <= score {
			return
		}
		kind, score, ok = k, s, true
		reason = reasonFor(k, hint, target)
	}

	for _, h := range hints {
		if strings.EqualFold(h, name) {
			consider(MatchExactName, scoreExactName, h, name)
		} else if Normalize(h) == normName {
			consider(MatchNormalizedName, scoreNormalizedName, h, name)
		}
		for _, a := range aliases {
			if strings.EqualFold(h, a) {
				consider(MatchExactAlias, scoreExactAlias, h, a)
			} else if Normalize(h) == Normalize(a) {
				consider(MatchNormalizedAlias, scoreNormalizedAlias, h, a)
			}
		}
	}
	return kind, reason, score, ok
}

func reasonFor(kind MatchKind, hint, target string) string {
	switch kind {
	case MatchExactName:
		return fmt.Sprintf("%q exactly matches entity name %q", hint, target)
	case MatchExactAlias:
		return fmt.Sprintf("%q exactly matches alias %q", hint, target)
	case MatchNormalizedName:
		return fmt.Sprintf("%q matches entity name %q after case/diacritic normalization", hint, target)
	case MatchNormalizedAlias:
		return fmt.Sprintf("%q matches alias %q after case/diacritic normalization", hint, target)
	default:
		return ""
	}
}
