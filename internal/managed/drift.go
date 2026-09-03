package managed

import (
	"encoding/json"
	"fmt"
	"net/http"
	"sort"
	"time"
)

// DriftStatus reports whether a manifest pin matches, trails, or could
// not be compared against the core's latest upstream release.
type DriftStatus string

const (
	DriftCurrent DriftStatus = "current"
	DriftBehind  DriftStatus = "behind"
	DriftUnknown DriftStatus = "unknown"
)

// DriftResult is the per-core outcome of comparing a manifest pin
// against its repo's latest release.
type DriftResult struct {
	Core   string
	Pinned string
	Latest string
	Status DriftStatus
	Reason string // set when Status == DriftUnknown
}

// LatestTagFunc resolves the latest release tag for a GitHub repo
// ("owner/name"). An error or empty tag means the latest release could
// not be determined (no releases, or a network/API failure) — the
// caller reports DriftUnknown rather than guessing DriftBehind.
type LatestTagFunc func(repo string) (string, error)

// CheckDrift compares every core's pinned version against its repo's
// latest release. Results are sorted by core name for deterministic
// output. A fetch error or empty tag is reported as DriftUnknown, never
// as drift — an outage or a rate-limited run must never read as
// "behind" (see the Risks note on internal/managed/manifest.json's
// consuming issue).
func CheckDrift(m *Manifest, latest LatestTagFunc) []DriftResult {
	names := make([]string, 0, len(m.Cores))
	for name := range m.Cores {
		names = append(names, name)
	}
	sort.Strings(names)

	results := make([]DriftResult, 0, len(names))
	for _, name := range names {
		core := m.Cores[name]
		tag, err := latest(core.Repo)
		if err != nil {
			results = append(results, DriftResult{
				Core: name, Pinned: core.Version, Status: DriftUnknown,
				Reason: err.Error(),
			})
			continue
		}
		if tag == "" {
			results = append(results, DriftResult{
				Core: name, Pinned: core.Version, Status: DriftUnknown,
				Reason: "no releases found",
			})
			continue
		}

		status := DriftCurrent
		if normalizeVersion(tag) != normalizeVersion(core.Version) {
			status = DriftBehind
		}
		results = append(results, DriftResult{
			Core: name, Pinned: core.Version, Latest: tag, Status: status,
		})
	}
	return results
}

// GitHubLatestTag returns a LatestTagFunc backed by the public GitHub
// REST API (GET /repos/{repo}/releases/latest). It is the production
// resolver; tests supply a fake LatestTagFunc instead of hitting the
// network. A nil client gets a default 10s-timeout client. An empty
// token issues unauthenticated requests (rate-limited, but sufficient
// for one request per core on a weekly schedule).
func GitHubLatestTag(client *http.Client, token string) LatestTagFunc {
	return githubLatestTagWithBaseURL(client, token, "https://api.github.com")
}

// githubLatestTagWithBaseURL is GitHubLatestTag with the API base URL
// overridable, so tests can point it at an httptest.Server instead of
// the real GitHub API.
func githubLatestTagWithBaseURL(client *http.Client, token, baseURL string) LatestTagFunc {
	if client == nil {
		client = &http.Client{Timeout: 10 * time.Second}
	}
	return func(repo string) (string, error) {
		req, err := http.NewRequest(http.MethodGet, baseURL+"/repos/"+repo+"/releases/latest", nil)
		if err != nil {
			return "", err
		}
		req.Header.Set("Accept", "application/vnd.github+json")
		if token != "" {
			req.Header.Set("Authorization", "Bearer "+token)
		}
		resp, err := client.Do(req)
		if err != nil {
			return "", err
		}
		defer resp.Body.Close()

		if resp.StatusCode == http.StatusNotFound {
			return "", nil // no releases published — DriftUnknown, not drift
		}
		if resp.StatusCode != http.StatusOK {
			return "", fmt.Errorf("github: unexpected status %d for %s", resp.StatusCode, repo)
		}

		var body struct {
			TagName string `json:"tag_name"`
		}
		if err := json.NewDecoder(resp.Body).Decode(&body); err != nil {
			return "", err
		}
		return body.TagName, nil
	}
}
