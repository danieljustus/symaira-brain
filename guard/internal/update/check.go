// Package update provides update checking for symguard using corekit/updatecheck.
package update

import (
	"context"
	"fmt"
	"time"

	"github.com/danieljustus/symaira-corekit/updatecheck"
)

const (
	owner = "danieljustus"
	repo  = "symaira-guard"

	// cacheTTL keeps repeated checks within one process cheap. The cache is
	// in-memory only, so it does not span CLI invocations — the request
	// timeout below is what bounds every fresh process.
	cacheTTL = 24 * time.Hour
)

// requestTimeout bounds the GitHub API request. A slow or unreachable GitHub
// must never stall a CLI command beyond this window.
var requestTimeout = 2 * time.Second

// Info holds the result of an update check.
type Info struct {
	Current string // installed version
	Latest  string // latest release tag (empty if up to date)
	URL     string // release URL (empty if up to date)
}

// Checker queries the latest release for the configured repository.
// It is an interface so the network path can be stubbed in tests.
type Checker interface {
	Check(ctx context.Context, currentVersion string) (*updatecheck.Release, error)
}

// newChecker builds the production checker backed by the GitHub API.
// It is a variable so tests can substitute a stub.
var newChecker = func() Checker {
	c := updatecheck.NewChecker(owner, repo)
	c.CacheTTL = cacheTTL
	return c
}

// Check queries the latest GitHub release and returns update info.
// Errors are silently swallowed — never block or disrupt the CLI.
func Check(ctx context.Context, currentVersion string) *Info {
	if currentVersion == "" || currentVersion == "dev" {
		return nil
	}

	ctx, cancel := context.WithTimeout(ctx, requestTimeout)
	defer cancel()

	release, err := newChecker().Check(ctx, currentVersion)
	if err != nil {
		return nil // silently swallow errors
	}
	if release == nil {
		return nil // up to date
	}

	return &Info{
		Current: currentVersion,
		Latest:  release.TagName,
		URL:     release.HTMLURL,
	}
}

// Format returns a human-readable update message for stderr,
// or empty string if the version is current.
func Format(info *Info) string {
	if info == nil {
		return ""
	}
	return fmt.Sprintf("symguard: update available: %s → %s\n  %s\n",
		info.Current, info.Latest, info.URL)
}
