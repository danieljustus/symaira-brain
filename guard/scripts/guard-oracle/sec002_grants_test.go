package main

import "testing"

func TestNormalizeStorePathUsesStableSeparator(t *testing.T) {
	for _, input := range []string{
		`parse <store-dir>/grants.json`,
		`parse <store-dir>\grants.json`,
	} {
		if got, want := normalizeStorePath(input, `C:\temp\sec002-grants`), `parse <store-dir>/grants.json`; got != want {
			t.Errorf("normalizeStorePath(%q) = %q, want %q", input, got, want)
		}
	}
}
