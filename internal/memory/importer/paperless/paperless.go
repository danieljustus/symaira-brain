package paperless

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
)

// PaperlessImporter imports documents from Paperless-ngx.
type PaperlessImporter struct {
	baseURL       string
	token         string
	tag           string // optional filter
	correspondent string // optional filter
	maxContent    int    // max chars of content preview
}

// paperlessDocument represents a document from Paperless API.
type paperlessDocument struct {
	ID            int    `json:"id"`
	Title         string `json:"title"`
	Content       string `json:"content"`
	CreatedDate   string `json:"created_date"`
	Added         string `json:"added"`
	Modified      string `json:"modified"`
	Correspondent struct {
		ID   int    `json:"id"`
		Name string `json:"name"`
	} `json:"correspondent"`
	Tags []struct {
		ID   int    `json:"id"`
		Name string `json:"name"`
	} `json:"tags"`
	FileType  string `json:"file_type"`
	PageCount int    `json:"page_count"`
}

// paperlessListResponse represents the Paperless API list response.
type paperlessListResponse struct {
	Count   int                 `json:"count"`
	Results []paperlessDocument `json:"results"`
	Next    string              `json:"next"`
}

// NewPaperlessImporter creates a new Paperless importer.
func NewPaperlessImporter(baseURL, token, tag, correspondent string, maxContent int) *PaperlessImporter {
	if maxContent <= 0 {
		maxContent = 1000
	}
	return &PaperlessImporter{
		baseURL:       baseURL,
		token:         token,
		tag:           tag,
		correspondent: correspondent,
		maxContent:    maxContent,
	}
}

func (p *PaperlessImporter) Name() string { return "paperless" }

func (p *PaperlessImporter) Category() string { return "documents" }

func (p *PaperlessImporter) PrivacyLevel() importer.PrivacyLevel {
	return importer.PrivacyConfidential
}

func (p *PaperlessImporter) RequiresPIIGuard() bool { return true }

// DiscoverSessions finds documents since the given time.
func (p *PaperlessImporter) DiscoverSessions(since time.Time) ([]importer.SessionRef, error) {
	if p.baseURL == "" {
		p.baseURL = os.Getenv("PAPERLESS_URL")
	}
	if p.token == "" {
		p.token = os.Getenv("PAPERLESS_TOKEN")
	}

	if p.baseURL == "" || p.token == "" {
		return nil, fmt.Errorf("PAPERLESS_URL and PAPERLESS_TOKEN must be set")
	}

	documents, err := p.fetchDocuments(since)
	if err != nil {
		return nil, err
	}

	var sessions []importer.SessionRef
	for _, doc := range documents {
		docTime, err := time.Parse("2006-01-02", doc.CreatedDate)
		if err != nil {
			docTime, _ = time.Parse(time.RFC3339, doc.Added)
		}

		session := importer.SessionRef{
			Tool:       "paperless",
			SessionID:  fmt.Sprintf("%d", doc.ID),
			Path:       fmt.Sprintf("%s/documents/%d/", p.baseURL, doc.ID),
			ModifiedAt: docTime,
			Metadata: map[string]string{
				"title":     doc.Title,
				"file_type": doc.FileType,
			},
		}

		if doc.Correspondent.Name != "" {
			session.Metadata["correspondent"] = doc.Correspondent.Name
		}

		if len(doc.Tags) > 0 {
			var tagNames []string
			for _, t := range doc.Tags {
				tagNames = append(tagNames, t.Name)
			}
			jsonTags, _ := json.Marshal(tagNames)
			session.Metadata["tags"] = string(jsonTags)
		}

		sessions = append(sessions, session)
	}

	return sessions, nil
}

// ImportSession imports a single document as facts.
func (p *PaperlessImporter) ImportSession(ref importer.SessionRef) ([]importer.ImportedFact, error) {
	docID := ref.SessionID

	doc, err := p.fetchDocument(docID)
	if err != nil {
		return nil, err
	}

	// Truncate content
	content := doc.Content
	if len(content) > p.maxContent {
		content = content[:p.maxContent] + "..."
	}

	// Build fact content
	factContent := fmt.Sprintf("Document: %s", doc.Title)
	if doc.Correspondent.Name != "" {
		factContent += fmt.Sprintf(" from %s", doc.Correspondent.Name)
	}
	if content != "" {
		factContent += "\n\n" + content
	}

	metadata := map[string]string{
		"document_id": fmt.Sprintf("%d", doc.ID),
		"title":       doc.Title,
		"source":      "paperless",
		"file_type":   doc.FileType,
	}

	docTime, _ := time.Parse("2006-01-02", doc.CreatedDate)
	if !docTime.IsZero() {
		metadata["document_date"] = docTime.Format("2006-01-02")
	}

	if doc.Correspondent.Name != "" {
		metadata["correspondent"] = doc.Correspondent.Name
	}

	if len(doc.Tags) > 0 {
		var tagNames []string
		for _, t := range doc.Tags {
			tagNames = append(tagNames, t.Name)
		}
		jsonTags, _ := json.Marshal(tagNames)
		metadata["tags"] = string(jsonTags)
	}

	if doc.PageCount > 0 {
		metadata["page_count"] = fmt.Sprintf("%d", doc.PageCount)
	}

	return []importer.ImportedFact{{
		Content:   factContent,
		Source:    "paperless",
		SessionID: ref.SessionID,
		Timestamp: docTime,
		Metadata:  metadata,
	}}, nil
}

// fetchDocuments fetches documents from Paperless API.
func (p *PaperlessImporter) fetchDocuments(since time.Time) ([]paperlessDocument, error) {
	url := fmt.Sprintf("%s/api/documents/?format=json&ordering=-created_date", p.baseURL)

	if !since.IsZero() {
		url += fmt.Sprintf("&created_date__gte=%s", since.Format("2006-01-02"))
	}

	if p.tag != "" {
		url += fmt.Sprintf("&tags__name=%s", p.tag)
	}

	if p.correspondent != "" {
		url += fmt.Sprintf("&correspondent__name=%s", p.correspondent)
	}

	var allDocuments []paperlessDocument
	for url != "" {
		var resp paperlessListResponse
		if err := p.doAPIRequest(url, &resp); err != nil {
			return nil, err
		}
		allDocuments = append(allDocuments, resp.Results...)

		if resp.Next != "" {
			url = resp.Next
		} else {
			break
		}
	}

	return allDocuments, nil
}

// fetchDocument fetches a single document by ID.
func (p *PaperlessImporter) fetchDocument(docID string) (*paperlessDocument, error) {
	url := fmt.Sprintf("%s/api/documents/%s/?format=json", p.baseURL, docID)

	var doc paperlessDocument
	if err := p.doAPIRequest(url, &doc); err != nil {
		return nil, err
	}

	return &doc, nil
}

// doAPIRequest performs an authenticated HTTP request to Paperless API.
func (p *PaperlessImporter) doAPIRequest(url string, result interface{}) error {
	req, err := http.NewRequest("GET", url, nil)
	if err != nil {
		return fmt.Errorf("failed to create request: %w", err)
	}

	req.Header.Set("Authorization", "Token "+p.token)
	req.Header.Set("Accept", "application/json")

	client := &http.Client{Timeout: 30 * time.Second}
	resp, err := client.Do(req)
	if err != nil {
		return fmt.Errorf("API request failed: %w", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != 200 {
		body, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("API returned status %d: %s", resp.StatusCode, string(body))
	}

	return json.NewDecoder(resp.Body).Decode(result)
}
