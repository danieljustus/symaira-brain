package entity

import "testing"

func TestNormalize(t *testing.T) {
	tests := []struct {
		name string
		in   string
		want string
	}{
		{"trims and lowercases", "  Alice  ", "alice"},
		{"collapses internal whitespace", "Guillaume   Charhon", "guillaume charhon"},
		{"strips diacritics", "Château", "chateau"},
		{"strips diacritics mixed case", "GUILLAUME ÉLÉONORE", "guillaume eleonore"},
		{"handles cedilla", "François", "francois"},
		{"empty stays empty", "", ""},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := Normalize(tt.in); got != tt.want {
				t.Errorf("Normalize(%q) = %q, want %q", tt.in, got, tt.want)
			}
		})
	}
}

func TestIsPII(t *testing.T) {
	tests := []struct {
		name string
		in   string
		want bool
	}{
		{"email", "guillaume@example.com", true},
		{"email mixed case", "Guillaume.Charhon+work@Example.CO", true},
		{"international phone", "+1 415 555 0132", true},
		{"phone with dashes", "415-555-0132", true},
		{"plain name", "Guillaume Charhon", false},
		{"short alias", "GC", false},
		{"single word alias", "Chuck", false},
		{"empty", "", false},
		{"numeric but short", "12345", false},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := IsPII(tt.in); got != tt.want {
				t.Errorf("IsPII(%q) = %v, want %v", tt.in, got, tt.want)
			}
		})
	}
}

func TestBestMatch_ExactNameOutranksNormalized(t *testing.T) {
	hints := []string{"chateau", "Château"}
	kind, reason, score, ok := BestMatch(hints, "Château", nil)
	if !ok {
		t.Fatal("expected a match")
	}
	if kind != MatchExactName {
		t.Errorf("kind = %s, want %s", kind, MatchExactName)
	}
	if score != scoreExactName {
		t.Errorf("score = %v, want %v", score, scoreExactName)
	}
	if reason == "" {
		t.Error("expected non-empty reason")
	}
}

func TestBestMatch_ExactNameOutranksExactAlias(t *testing.T) {
	kind, _, score, ok := BestMatch([]string{"Alice"}, "Alice", []string{"Alice"})
	if !ok || kind != MatchExactName || score != scoreExactName {
		t.Fatalf("expected exact_name match, got kind=%s score=%v ok=%v", kind, score, ok)
	}
}

func TestBestMatch_ExactAlias(t *testing.T) {
	kind, reason, score, ok := BestMatch([]string{"Ali"}, "Alice", []string{"Ali", "Al"})
	if !ok {
		t.Fatal("expected a match")
	}
	if kind != MatchExactAlias || score != scoreExactAlias {
		t.Fatalf("kind = %s score = %v, want %s %v", kind, score, MatchExactAlias, scoreExactAlias)
	}
	if reason == "" {
		t.Error("expected non-empty reason")
	}
}

func TestBestMatch_NormalizedAlias(t *testing.T) {
	kind, _, score, ok := BestMatch([]string{"francois"}, "Someone Else", []string{"François"})
	if !ok || kind != MatchNormalizedAlias || score != scoreNormalizedAlias {
		t.Fatalf("kind = %s score = %v ok=%v, want %s %v true", kind, score, ok, MatchNormalizedAlias, scoreNormalizedAlias)
	}
}

func TestBestMatch_NoMatch(t *testing.T) {
	_, _, _, ok := BestMatch([]string{"Nobody"}, "Alice", []string{"Ali"})
	if ok {
		t.Fatal("expected no match")
	}
}

func TestBestMatch_UnicodeDiacriticAndCase(t *testing.T) {
	kind, _, _, ok := BestMatch([]string{"GUILLAUME CHARHON"}, "Guillaume Chárhon", nil)
	if !ok || kind != MatchNormalizedName {
		t.Fatalf("kind = %s ok=%v, want %s true", kind, ok, MatchNormalizedName)
	}
}

func TestBestMatch_DuplicateAliasesDoNotChangeOutcome(t *testing.T) {
	kind1, _, score1, ok1 := BestMatch([]string{"Ali"}, "Alice", []string{"Ali", "Ali", "Al"})
	kind2, _, score2, ok2 := BestMatch([]string{"Ali"}, "Alice", []string{"Ali", "Al"})
	if !ok1 || !ok2 || kind1 != kind2 || score1 != score2 {
		t.Fatalf("duplicate aliases changed outcome: (%s,%v,%v) vs (%s,%v,%v)", kind1, score1, ok1, kind2, score2, ok2)
	}
}

func TestBestMatch_EmptyInputsNoMatch(t *testing.T) {
	_, _, _, ok := BestMatch(nil, "Alice", []string{"Ali"})
	if ok {
		t.Fatal("expected no match with empty hints")
	}
}

func TestBestMatch_OversizedHintStillComparesSafely(t *testing.T) {
	huge := make([]byte, 5000)
	for i := range huge {
		huge[i] = 'a'
	}
	_, _, _, ok := BestMatch([]string{string(huge)}, "Alice", []string{"Ali"})
	if ok {
		t.Fatal("expected no match for an oversized, unrelated hint")
	}
}

func TestBestMatch_FirstHintWinsOnTie(t *testing.T) {
	// Both hints exactly match the name; the first one in input order must
	// determine the reported reason, keeping output deterministic.
	kind, reason, _, ok := BestMatch([]string{"Alice", "ALICE"}, "Alice", nil)
	if !ok || kind != MatchExactName {
		t.Fatalf("expected exact_name match, got kind=%s ok=%v", kind, ok)
	}
	if reason != `"Alice" exactly matches entity name "Alice"` {
		t.Errorf("reason = %q, want first hint to win", reason)
	}
}
