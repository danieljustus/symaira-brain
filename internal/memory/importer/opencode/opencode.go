package opencode

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/consolidation"
	"github.com/danieljustus/symaira-brain/internal/memory/importer"
	_ "modernc.org/sqlite"
)

// OpenCodeImporter imports sessions from OpenCode's SQLite database.
type OpenCodeImporter struct {
	customPath string
}

// sessionRow represents a row from the session table.
type sessionRow struct {
	ID          string
	Title       string
	Directory   string
	TimeCreated int64 // epoch ms
	TimeUpdated int64 // epoch ms
	Agent       string
	Model       string // JSON string
}

// messageData is the JSON structure stored in the message.data column.
type messageData struct {
	Role       string      `json:"role"`
	Time       messageTime `json:"time"`
	Tokens     *tokenInfo  `json:"tokens,omitempty"`
	ModelID    string      `json:"modelID,omitempty"`
	ProviderID string      `json:"providerID,omitempty"`
	Agent      string      `json:"agent,omitempty"`
}

type messageTime struct {
	Created   int64 `json:"created"`
	Completed int64 `json:"completed,omitempty"`
}

type tokenInfo struct {
	Input  int `json:"input"`
	Output int `json:"output"`
}

// partData is the JSON structure stored in the part.data column.
type partData struct {
	Type string `json:"type"`
	Text string `json:"text,omitempty"`
	Tool string `json:"tool,omitempty"`
}

func NewOpenCodeImporter(customPath string) *OpenCodeImporter {
	return &OpenCodeImporter{customPath: customPath}
}

func (o *OpenCodeImporter) Name() string {
	return "opencode"
}

func (o *OpenCodeImporter) Category() string {
	return "code"
}

func (o *OpenCodeImporter) PrivacyLevel() importer.PrivacyLevel {
	return importer.PrivacyInternal
}

func (o *OpenCodeImporter) RequiresPIIGuard() bool {
	return true
}

func (o *OpenCodeImporter) IsTranscript() bool { return true }

func (o *OpenCodeImporter) dbPath() string {
	if o.customPath != "" {
		return o.customPath
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return ""
	}
	return filepath.Join(home, ".local", "share", "opencode", "opencode.db")
}

func (o *OpenCodeImporter) DiscoverSessions(since time.Time) ([]importer.SessionRef, error) {
	dbPath := o.dbPath()
	if dbPath == "" {
		return nil, nil
	}

	if _, err := os.Stat(dbPath); os.IsNotExist(err) {
		return nil, nil
	}

	db, err := sql.Open("sqlite", dbPath)
	if err != nil {
		return nil, fmt.Errorf("failed to open opencode database: %w", err)
	}
	defer func() { _ = db.Close() }()

	// Query sessions modified since the given time.
	// time_updated is stored as epoch milliseconds.
	sinceMs := since.UnixMilli()

	rows, err := db.Query(`
		SELECT id, title, directory, time_created, time_updated, agent, model
		FROM session
		WHERE time_updated > ?
		ORDER BY time_updated DESC
	`, sinceMs)
	if err != nil {
		return nil, fmt.Errorf("failed to query sessions: %w", err)
	}
	defer func() { _ = rows.Close() }()

	var sessions []importer.SessionRef
	for rows.Next() {
		var s sessionRow
		if err := rows.Scan(&s.ID, &s.Title, &s.Directory, &s.TimeCreated, &s.TimeUpdated, &s.Agent, &s.Model); err != nil {
			continue
		}

		modifiedAt := time.UnixMilli(s.TimeUpdated)
		metadata := map[string]string{
			"title":     s.Title,
			"directory": s.Directory,
			"agent":     s.Agent,
			// #483: coding-session material → code prompt family.
			consolidation.PromptModeKey: consolidation.PromptFamilyCode,
		}
		if s.Model != "" {
			metadata["model"] = s.Model
		}

		sessions = append(sessions, importer.SessionRef{
			Tool:       "opencode",
			SessionID:  s.ID,
			Path:       dbPath,
			ModifiedAt: modifiedAt,
			Metadata:   metadata,
		})
	}

	return sessions, rows.Err()
}

func (o *OpenCodeImporter) ImportSession(ref importer.SessionRef) ([]importer.ImportedFact, error) {
	db, err := sql.Open("sqlite", ref.Path)
	if err != nil {
		return nil, fmt.Errorf("failed to open opencode database: %w", err)
	}
	defer func() { _ = db.Close() }()

	// Get session metadata for enrichment.
	var title, directory, agent string
	err = db.QueryRow(`
		SELECT title, directory, agent FROM session WHERE id = ?
	`, ref.SessionID).Scan(&title, &directory, &agent)
	if err != nil {
		return nil, fmt.Errorf("failed to query session: %w", err)
	}

	// Query messages for this session, ordered by creation time.
	rows, err := db.Query(`
		SELECT id, data FROM message
		WHERE session_id = ?
		ORDER BY time_created ASC
	`, ref.SessionID)
	if err != nil {
		return nil, fmt.Errorf("failed to query messages: %w", err)
	}
	defer func() { _ = rows.Close() }()

	var facts []importer.ImportedFact

	for rows.Next() {
		var msgID, msgDataJSON string
		if err := rows.Scan(&msgID, &msgDataJSON); err != nil {
			continue
		}

		var msgData messageData
		if err := json.Unmarshal([]byte(msgDataJSON), &msgData); err != nil {
			continue
		}

		// Only extract facts from assistant messages.
		if msgData.Role != "assistant" {
			continue
		}

		// Get the text content from parts belonging to this message.
		textContent, err := o.getMessageText(db, msgID)
		if err != nil || textContent == "" {
			continue
		}

		// Skip very short responses (likely just acknowledgments).
		if len(textContent) < 50 {
			continue
		}

		var timestamp time.Time
		if msgData.Time.Created > 0 {
			timestamp = time.UnixMilli(msgData.Time.Created)
		}

		model := ""
		if msgData.ModelID != "" {
			model = msgData.ModelID
			if msgData.ProviderID != "" {
				model = msgData.ProviderID + "/" + msgData.ModelID
			}
		}

		facts = append(facts, importer.ImportedFact{
			Content:   textContent,
			Source:    "opencode",
			SessionID: ref.SessionID,
			Timestamp: timestamp,
			Metadata: map[string]string{
				"title":      title,
				"directory":  directory,
				"agent":      agent,
				"model":      model,
				"message_id": msgID,
			},
		})
	}

	return facts, rows.Err()
}

// getMessageText retrieves the concatenated text content from all text-type
// parts belonging to a given message.
func (o *OpenCodeImporter) getMessageText(db *sql.DB, messageID string) (string, error) {
	rows, err := db.Query(`
		SELECT data FROM part
		WHERE message_id = ? AND json_extract(data, '$.type') = 'text'
		ORDER BY time_created ASC
	`, messageID)
	if err != nil {
		return "", err
	}
	defer func() { _ = rows.Close() }()

	var texts []string
	for rows.Next() {
		var partJSON string
		if err := rows.Scan(&partJSON); err != nil {
			continue
		}

		var pd partData
		if err := json.Unmarshal([]byte(partJSON), &pd); err != nil {
			continue
		}

		text := strings.TrimSpace(pd.Text)
		if text != "" {
			texts = append(texts, text)
		}
	}

	return strings.Join(texts, "\n"), rows.Err()
}
