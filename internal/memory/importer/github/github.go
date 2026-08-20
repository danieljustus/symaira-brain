package github

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
)

type GitHubImporter struct {
	owner string
	repo  string
	token string
}

type ghPR struct {
	Number    int      `json:"number"`
	Title     string   `json:"title"`
	Body      string   `json:"body"`
	State     string   `json:"state"`
	Author    string   `json:"author"`
	CreatedAt string   `json:"createdAt"`
	MergedAt  string   `json:"mergedAt"`
	URL       string   `json:"url"`
	Labels    []string `json:"labels"`
}

type ghIssue struct {
	Number    int      `json:"number"`
	Title     string   `json:"title"`
	Body      string   `json:"body"`
	State     string   `json:"state"`
	Author    string   `json:"author"`
	CreatedAt string   `json:"createdAt"`
	ClosedAt  string   `json:"closedAt"`
	URL       string   `json:"url"`
	Labels    []string `json:"labels"`
}

func NewGitHubImporter(owner, repo, token string) *GitHubImporter {
	return &GitHubImporter{owner: owner, repo: repo, token: token}
}

func (g *GitHubImporter) Name() string { return "github" }

func (g *GitHubImporter) Category() string { return "code" }

func (g *GitHubImporter) PrivacyLevel() importer.PrivacyLevel { return importer.PrivacyInternal }

func (g *GitHubImporter) RequiresPIIGuard() bool { return false }

func (g *GitHubImporter) DiscoverSessions(since time.Time) ([]importer.SessionRef, error) {
	var sessions []importer.SessionRef

	prSessions, err := g.discoverPRs(since)
	if err != nil {
		fmt.Fprintf(g.stderr(), "Warning: failed to discover PRs: %v\n", err)
	} else {
		sessions = append(sessions, prSessions...)
	}

	issueSessions, err := g.discoverIssues(since)
	if err != nil {
		fmt.Fprintf(g.stderr(), "Warning: failed to discover issues: %v\n", err)
	} else {
		sessions = append(sessions, issueSessions...)
	}

	return sessions, nil
}

func (g *GitHubImporter) discoverPRs(since time.Time) ([]importer.SessionRef, error) {
	sinceStr := since.Format("2006-01-02T15:04:05Z")
	query := fmt.Sprintf("repo:%s/%s is:pr created:>=%s", g.owner, g.repo, sinceStr)

	out, err := exec.Command("gh", "pr", "list",
		"--repo", g.owner+"/"+g.repo,
		"--search", query,
		"--state", "all",
		"--limit", "200",
		"--json", "number,title,body,state,author,createdAt,mergedAt,url,labels",
	).Output()
	if err != nil {
		return nil, fmt.Errorf("gh pr list failed: %w", err)
	}

	var prs []ghPR
	if err := json.Unmarshal(out, &prs); err != nil {
		return nil, fmt.Errorf("failed to parse PR JSON: %w", err)
	}

	var sessions []importer.SessionRef
	for _, pr := range prs {
		created, _ := time.Parse(time.RFC3339Nano, pr.CreatedAt)

		sessions = append(sessions, importer.SessionRef{
			Tool:       "github",
			SessionID:  fmt.Sprintf("pr/%d", pr.Number),
			Path:       pr.URL,
			ModifiedAt: created,
			Metadata: map[string]string{
				"type":   "pr",
				"state":  pr.State,
				"title":  pr.Title,
				"author": pr.Author,
			},
		})
	}

	return sessions, nil
}

func (g *GitHubImporter) discoverIssues(since time.Time) ([]importer.SessionRef, error) {
	sinceStr := since.Format("2006-01-02T15:04:05Z")
	query := fmt.Sprintf("repo:%s/%s is:issue created:>=%s", g.owner, g.repo, sinceStr)

	out, err := exec.Command("gh", "issue", "list",
		"--repo", g.owner+"/"+g.repo,
		"--search", query,
		"--state", "all",
		"--limit", "200",
		"--json", "number,title,body,state,author,createdAt,closedAt,url,labels",
	).Output()
	if err != nil {
		return nil, fmt.Errorf("gh issue list failed: %w", err)
	}

	var issues []ghIssue
	if err := json.Unmarshal(out, &issues); err != nil {
		return nil, fmt.Errorf("failed to parse issue JSON: %w", err)
	}

	var sessions []importer.SessionRef
	for _, issue := range issues {
		created, _ := time.Parse(time.RFC3339Nano, issue.CreatedAt)

		sessions = append(sessions, importer.SessionRef{
			Tool:       "github",
			SessionID:  fmt.Sprintf("issue/%d", issue.Number),
			Path:       issue.URL,
			ModifiedAt: created,
			Metadata: map[string]string{
				"type":   "issue",
				"state":  issue.State,
				"title":  issue.Title,
				"author": issue.Author,
			},
		})
	}

	return sessions, nil
}

func (g *GitHubImporter) ImportSession(ref importer.SessionRef) ([]importer.ImportedFact, error) {
	parts := strings.SplitN(ref.SessionID, "/", 2)
	if len(parts) != 2 {
		return nil, fmt.Errorf("invalid session ID format: %s", ref.SessionID)
	}

	itemType := parts[0]
	number := parts[1]

	var out []byte
	var err error

	switch itemType {
	case "pr":
		out, err = exec.Command("gh", "pr", "view", number,
			"--repo", g.owner+"/"+g.repo,
			"--json", "number,title,body,state,author,createdAt,mergedAt,url,labels,files",
		).Output()
	case "issue":
		out, err = exec.Command("gh", "issue", "view", number,
			"--repo", g.owner+"/"+g.repo,
			"--json", "number,title,body,state,author,createdAt,closedAt,url,labels,comments",
		).Output()
	default:
		return nil, fmt.Errorf("unknown item type: %s", itemType)
	}

	if err != nil {
		return nil, fmt.Errorf("gh view failed for %s: %w", ref.SessionID, err)
	}

	var facts []importer.ImportedFact

	switch itemType {
	case "pr":
		var pr ghPR
		if err := json.Unmarshal(out, &pr); err != nil {
			return nil, fmt.Errorf("failed to parse PR JSON: %w", err)
		}

		content := fmt.Sprintf("PR #%d: %s", pr.Number, pr.Title)
		if pr.Body != "" {
			body := pr.Body
			if len(body) > 2000 {
				body = body[:2000] + "..."
			}
			content += "\n\n" + body
		}

		created, _ := time.Parse(time.RFC3339Nano, pr.CreatedAt)

		facts = append(facts, importer.ImportedFact{
			Content:   content,
			Source:    "github",
			SessionID: ref.SessionID,
			Timestamp: created,
			Metadata: map[string]string{
				"type":  "pr",
				"state": pr.State,
				"url":   pr.URL,
			},
		})

	case "issue":
		var issue ghIssue
		if err := json.Unmarshal(out, &issue); err != nil {
			return nil, fmt.Errorf("failed to parse issue JSON: %w", err)
		}

		content := fmt.Sprintf("Issue #%d: %s", issue.Number, issue.Title)
		if issue.Body != "" {
			body := issue.Body
			if len(body) > 2000 {
				body = body[:2000] + "..."
			}
			content += "\n\n" + body
		}

		created, _ := time.Parse(time.RFC3339Nano, issue.CreatedAt)

		facts = append(facts, importer.ImportedFact{
			Content:   content,
			Source:    "github",
			SessionID: ref.SessionID,
			Timestamp: created,
			Metadata: map[string]string{
				"type":  "issue",
				"state": issue.State,
				"url":   issue.URL,
			},
		})
	}

	return facts, nil
}

func (g *GitHubImporter) stderr() *os.File {
	return os.Stderr
}
