package email

import (
	"encoding/json"
	"fmt"
	"os/exec"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
)

// EmailImporter imports emails via the himalaya CLI.
type EmailImporter struct {
	folder         string
	importance     string // "high", "low", "" (all)
	maxBody        int    // max chars to import
	excludeSenders []string
	includeSenders []string
}

// himalayaMessage represents a message from himalaya's JSON output.
type himalayaMessage struct {
	ID        string   `json:"id"`
	Subject   string   `json:"subject"`
	From      string   `json:"from"`
	To        []string `json:"to"`
	Date      string   `json:"date"`
	Folder    string   `json:"folder"`
	Flags     []string `json:"flags"`
	HasAttach bool     `json:"hasAttachment"`
}

// himalayaBody represents the body response from himalaya.
type himalayaBody struct {
	Text string `json:"text"`
}

// NewEmailImporter creates a new email importer.
func NewEmailImporter(folder, importance string, maxBody int) *EmailImporter {
	if maxBody <= 0 {
		maxBody = 2000
	}
	return &EmailImporter{
		folder:     folder,
		importance: importance,
		maxBody:    maxBody,
	}
}

func (e *EmailImporter) Name() string { return "email" }

func (e *EmailImporter) Category() string { return "communication" }

func (e *EmailImporter) PrivacyLevel() importer.PrivacyLevel {
	return importer.PrivacyConfidential
}

func (e *EmailImporter) RequiresPIIGuard() bool { return true }

// DiscoverSessions finds emails since the given time.
func (e *EmailImporter) DiscoverSessions(since time.Time) ([]importer.SessionRef, error) {
	folder := e.folder
	if folder == "" {
		folder = "INBOX"
	}

	// Use himalaya to list messages
	args := []string{"message", "list", "--folder", folder, "--json"}

	out, err := exec.Command("himalaya", args...).Output()
	if err != nil {
		return nil, fmt.Errorf("himalaya list failed: %w", err)
	}

	var messages []himalayaMessage
	if err := json.Unmarshal(out, &messages); err != nil {
		return nil, fmt.Errorf("failed to parse himalaya output: %w", err)
	}

	var sessions []importer.SessionRef
	for _, msg := range messages {
		msgTime, err := time.Parse(time.RFC3339, msg.Date)
		if err != nil {
			continue
		}

		if msgTime.Before(since) {
			continue
		}

		// Filter by importance
		if e.importance != "" {
			hasFlag := false
			for _, flag := range msg.Flags {
				if strings.EqualFold(flag, e.importance) {
					hasFlag = true
					break
				}
			}
			if !hasFlag {
				continue
			}
		}

		// Filter by sender
		if !e.matchesSender(msg.From) {
			continue
		}

		session := importer.SessionRef{
			Tool:       "email",
			SessionID:  msg.ID,
			Path:       fmt.Sprintf("email://%s/%s", folder, msg.ID),
			ModifiedAt: msgTime,
			Metadata: map[string]string{
				"subject": msg.Subject,
				"from":    msg.From,
				"folder":  folder,
			},
		}

		sessions = append(sessions, session)
	}

	return sessions, nil
}

// ImportSession imports a single email as facts.
func (e *EmailImporter) ImportSession(ref importer.SessionRef) ([]importer.ImportedFact, error) {
	folder := ref.Metadata["folder"]
	if folder == "" {
		folder = "INBOX"
	}

	// Get message body
	args := []string{"message", "get", "--folder", folder, "--json", ref.SessionID}

	out, err := exec.Command("himalaya", args...).Output()
	if err != nil {
		return nil, fmt.Errorf("himalaya get failed: %w", err)
	}

	var body himalayaBody
	if err := json.Unmarshal(out, &body); err != nil {
		return nil, fmt.Errorf("failed to parse himalaya body: %w", err)
	}

	// Truncate body
	content := body.Text
	if len(content) > e.maxBody {
		content = content[:e.maxBody] + "..."
	}

	// Build fact content
	subject := ref.Metadata["subject"]
	from := ref.Metadata["from"]

	factContent := fmt.Sprintf("Email from %s: %s", from, subject)
	if content != "" {
		factContent += "\n\n" + content
	}

	// Parse timestamp
	ts, _ := time.Parse(time.RFC3339, ref.Metadata["date"])

	metadata := map[string]string{
		"message_id": ref.SessionID,
		"from":       from,
		"subject":    subject,
		"folder":     folder,
		"source":     "email",
	}

	return []importer.ImportedFact{{
		Content:   factContent,
		Source:    "email",
		SessionID: ref.SessionID,
		Timestamp: ts,
		Metadata:  metadata,
	}}, nil
}

// matchesSender checks if an email sender matches the include/exclude filters.
func (e *EmailImporter) matchesSender(from string) bool {
	if len(e.includeSenders) > 0 {
		for _, pattern := range e.includeSenders {
			if strings.Contains(from, pattern) {
				return true
			}
		}
		return false
	}

	for _, pattern := range e.excludeSenders {
		if strings.Contains(from, pattern) {
			return false
		}
	}

	return true
}
