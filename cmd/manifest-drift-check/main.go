// Command manifest-drift-check compares every pinned core version in
// internal/managed/manifest.json against its repo's latest GitHub
// release and reports drift. It never bumps the manifest itself —
// Core.SHA256 requires a bump to independently download and hash the
// release archive, so an automated pin change would be a downgrade in
// trust. This command only reports; a human bumps.
package main

import (
	"fmt"
	"os"

	"github.com/danieljustus/symaira-brain/internal/managed"
)

func main() {
	os.Exit(run())
}

func run() int {
	manifest, err := managed.LoadManifest()
	if err != nil {
		fmt.Fprintf(os.Stderr, "manifest-drift-check: %v\n", err)
		return 2
	}

	latest := managed.GitHubLatestTag(nil, os.Getenv("GITHUB_TOKEN"))
	results := managed.CheckDrift(manifest, latest)

	var behind, unknown int
	for _, r := range results {
		switch r.Status {
		case managed.DriftBehind:
			fmt.Printf("BEHIND   %-12s pinned=%-12s latest=%s\n", r.Core, r.Pinned, r.Latest)
			behind++
		case managed.DriftUnknown:
			fmt.Printf("UNKNOWN  %-12s pinned=%-12s (%s)\n", r.Core, r.Pinned, r.Reason)
			unknown++
		default:
			fmt.Printf("CURRENT  %-12s pinned=%s\n", r.Core, r.Pinned)
		}
	}

	if unknown > 0 {
		fmt.Fprintf(os.Stderr, "manifest-drift-check: %d core(s) could not be checked (network/rate-limit) — not treated as drift\n", unknown)
	}
	if behind > 0 {
		fmt.Fprintf(os.Stderr, "manifest-drift-check: %d core(s) behind their latest release\n", behind)
		return 1
	}
	return 0
}
