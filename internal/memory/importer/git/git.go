package git

import (
	"encoding/json"
	"fmt"
	"os/exec"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
)

type GitImporter struct {
	repoPath string
	author   string
}

type gitCommit struct {
	Hash       string `json:"hash"`
	Author     string `json:"author"`
	Email      string `json:"email"`
	Date       string `json:"date"`
	Message    string `json:"message"`
	Parents    string `json:"parents"`
	Files      []string
	Insertions int
	Deletions  int
}

func NewGitImporter(repoPath, author string) *GitImporter {
	return &GitImporter{repoPath: repoPath, author: author}
}

func (g *GitImporter) Name() string { return "git" }

func (g *GitImporter) Category() string { return "code" }

func (g *GitImporter) PrivacyLevel() importer.PrivacyLevel { return importer.PrivacyPublic }

func (g *GitImporter) RequiresPIIGuard() bool { return false }

func (g *GitImporter) DiscoverSessions(since time.Time) ([]importer.SessionRef, error) {
	sinceStr := since.Format("2006-01-02")

	args := []string{"-C", g.repoPath, "log", "--since=" + sinceStr, "--format=%H|%an|%ae|%aI|%s|%P", "--no-merges"}
	if g.author != "" {
		args = append(args, "--author="+g.author)
	}

	out, err := exec.Command("git", args...).Output()
	if err != nil {
		return nil, fmt.Errorf("git log failed: %w", err)
	}

	var sessions []importer.SessionRef
	lines := strings.Split(strings.TrimSpace(string(out)), "\n")

	for _, line := range lines {
		if line == "" {
			continue
		}
		parts := strings.SplitN(line, "|", 6)
		if len(parts) < 5 {
			continue
		}

		hash := parts[0]
		authorName := parts[1]
		dateStr := parts[3]
		subject := parts[4]

		date, _ := time.Parse(time.RFC3339Nano, dateStr)

		sessions = append(sessions, importer.SessionRef{
			Tool:       "git",
			SessionID:  hash,
			Path:       g.repoPath,
			ModifiedAt: date,
			Metadata: map[string]string{
				"author":  authorName,
				"subject": subject,
			},
		})
	}

	return sessions, nil
}

func (g *GitImporter) ImportSession(ref importer.SessionRef) ([]importer.ImportedFact, error) {
	hash := ref.SessionID

	format := `{"hash":"%H","author":"%an","email":"%ae","date":"%aI","message":"%s","parents":"%P"}`
	args := []string{"-C", g.repoPath, "log", "-1", "--format=" + format, hash}

	out, err := exec.Command("git", args...).Output()
	if err != nil {
		return nil, fmt.Errorf("git log failed for %s: %w", hash, err)
	}

	var commit gitCommit
	if err := json.Unmarshal(out, &commit); err != nil {
		return nil, fmt.Errorf("failed to parse commit JSON: %w", err)
	}

	diffArgs := []string{"-C", g.repoPath, "diff", "--stat", "--numstat", hash + "^.." + hash}
	diffOut, err := exec.Command("git", diffArgs...).Output()
	if err == nil {
		lines := strings.Split(strings.TrimSpace(string(diffOut)), "\n")
		for _, line := range lines {
			nums := strings.Fields(line)
			if len(nums) >= 3 {
				// Best-effort stat parsing: unparsable numbers stay zero,
				// which the caller treats as "no insertions/deletions".
				_, _ = fmt.Sscanf(nums[0], "%d", &commit.Insertions)
				_, _ = fmt.Sscanf(nums[1], "%d", &commit.Deletions)
			}
			if idx := strings.LastIndex(line, "\t"); idx >= 0 {
				commit.Files = append(commit.Files, line[idx+1:])
			}
		}
	}

	date, _ := time.Parse(time.RFC3339Nano, commit.Date)

	content := fmt.Sprintf("Commit %s by %s: %s", hash[:7], commit.Author, commit.Message)

	metadata := map[string]string{
		"commit":     hash,
		"author":     commit.Author,
		"email":      commit.Email,
		"date":       commit.Date,
		"insertions": fmt.Sprintf("%d", commit.Insertions),
		"deletions":  fmt.Sprintf("%d", commit.Deletions),
	}
	if len(commit.Files) > 0 {
		filesJSON, _ := json.Marshal(commit.Files)
		metadata["files_changed"] = string(filesJSON)
	}
	if strings.Contains(commit.Parents, " ") {
		metadata["merge"] = "true"
	}

	return []importer.ImportedFact{{
		Content:   content,
		Source:    "git",
		SessionID: ref.SessionID,
		Timestamp: date,
		Metadata:  metadata,
	}}, nil
}
