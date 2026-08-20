package paperless

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
)

func TestNewPaperlessImporter(t *testing.T) {
	imp := NewPaperlessImporter("http://localhost:8000", "token123", "", "", 1000)

	if imp.Name() != "paperless" {
		t.Errorf("Name() = %q, want %q", imp.Name(), "paperless")
	}

	if imp.Category() != "documents" {
		t.Errorf("Category() = %q, want %q", imp.Category(), "documents")
	}

	if imp.PrivacyLevel() != "confidential" {
		t.Errorf("PrivacyLevel() = %q, want %q", imp.PrivacyLevel(), "confidential")
	}

	if !imp.RequiresPIIGuard() {
		t.Error("RequiresPIIGuard() = false, want true")
	}
}

func TestNewPaperlessImporterDefaults(t *testing.T) {
	imp := NewPaperlessImporter("", "", "", "", 0)

	if imp.maxContent != 1000 {
		t.Errorf("maxContent = %d, want %d", imp.maxContent, 1000)
	}
}

func TestDiscoverSessionsNoConfig(t *testing.T) {
	imp := NewPaperlessImporter("", "", "", "", 1000)

	_, err := imp.DiscoverSessions(time.Now())
	if err == nil {
		t.Fatal("expected error with empty config")
	}
	if !strings.Contains(err.Error(), "PAPERLESS_URL") {
		t.Errorf("expected error about PAPERLESS_URL, got %q", err.Error())
	}
}

func TestDiscoverSessionsConnectionError(t *testing.T) {
	imp := NewPaperlessImporter("http://127.0.0.1:1", "test-token", "", "", 1000)

	_, err := imp.DiscoverSessions(time.Now())
	if err == nil {
		t.Fatal("expected error from connection failure")
	}
}

func TestImportSessionConnectionError(t *testing.T) {
	imp := NewPaperlessImporter("http://127.0.0.1:1", "test-token", "", "", 1000)
	ref := importer.SessionRef{SessionID: "999"}

	_, err := imp.ImportSession(ref)
	if err == nil {
		t.Fatal("expected error from connection failure")
	}
}

func TestDiscoverSessionsWithServerListResponse(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if strings.Contains(r.URL.Path, "/api/documents/") && r.URL.Path != "/api/documents/" {
			docID := strings.TrimPrefix(r.URL.Path, "/api/documents/")
			docID = strings.TrimSuffix(docID, "/")
			_, _ = w.Write([]byte(`{
				"id": ` + docID + `,
				"title": "Test Document",
				"content": "content",
				"created_date": "2026-06-15",
				"added": "2026-06-15T10:00:00Z",
				"correspondent": {"id": 1, "name": "Test Corp"},
				"tags": [{"id": 1, "name": "invoice"}],
				"file_type": "pdf",
				"page_count": 5
			}`))
			return
		}
		_, _ = w.Write([]byte(`{
			"count": 2,
			"next": null,
			"results": [
				{
					"id": 1,
					"title": "First Document",
					"content": "Content of first document.",
					"created_date": "2026-06-15",
					"added": "2026-06-15T10:00:00Z",
					"correspondent": {"id": 1, "name": "Test Corp"},
					"tags": [{"id": 1, "name": "invoice"}],
					"file_type": "pdf",
					"page_count": 3
				},
				{
					"id": 2,
					"title": "Second Document",
					"content": "",
					"created_date": "2026-06-16",
					"added": "2026-06-16T12:00:00Z",
					"correspondent": null,
					"tags": [],
					"file_type": "png",
					"page_count": 0
				}
			]
		}`))
	}))
	defer server.Close()

	imp := NewPaperlessImporter(server.URL, "test-token", "", "", 1000)
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}

	if len(sessions) != 2 {
		t.Fatalf("expected 2 sessions, got %d", len(sessions))
	}

	if sessions[0].Metadata["title"] != "First Document" {
		t.Errorf("title = %q, want %q", sessions[0].Metadata["title"], "First Document")
	}
	if sessions[0].Metadata["correspondent"] != "Test Corp" {
		t.Errorf("correspondent = %q, want %q", sessions[0].Metadata["correspondent"], "Test Corp")
	}
	if sessions[0].Metadata["tags"] == "" {
		t.Error("expected tags metadata for first document")
	}
	if sessions[0].Metadata["file_type"] != "pdf" {
		t.Errorf("file_type = %q, want %q", sessions[0].Metadata["file_type"], "pdf")
	}

	if sessions[1].Metadata["title"] != "Second Document" {
		t.Errorf("title = %q, want %q", sessions[1].Metadata["title"], "Second Document")
	}
	if _, ok := sessions[1].Metadata["correspondent"]; ok {
		t.Error("expected no correspondent for second document")
	}
	if _, ok := sessions[1].Metadata["tags"]; ok {
		t.Error("expected no tags for second document")
	}
}

func TestDiscoverSessionsPagination(t *testing.T) {
	callCount := 0
	var server *httptest.Server
	server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		callCount++
		if callCount == 1 {
			_, _ = w.Write([]byte(`{
				"count": 3,
				"next": "` + server.URL + `/api/documents/?page=2",
				"results": [
					{"id": 1, "title": "Doc 1", "content": "Content 1", "created_date": "2026-06-15", "added": "2026-06-15T10:00:00Z", "file_type": "pdf", "page_count": 1}
				]
			}`))
		} else {
			_, _ = w.Write([]byte(`{
				"count": 3,
				"next": null,
				"results": [
					{"id": 2, "title": "Doc 2", "content": "Content 2", "created_date": "2026-06-16", "added": "2026-06-16T10:00:00Z", "file_type": "pdf", "page_count": 1},
					{"id": 3, "title": "Doc 3", "content": "Content 3", "created_date": "2026-06-17", "added": "2026-06-17T10:00:00Z", "file_type": "pdf", "page_count": 1}
				]
			}`))
		}
	}))
	defer server.Close()

	imp := NewPaperlessImporter(server.URL, "test-token", "", "", 1000)
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}
	if len(sessions) != 3 {
		t.Fatalf("expected 3 sessions (across pages), got %d", len(sessions))
	}
	if callCount != 2 {
		t.Errorf("expected 2 API calls for pagination, got %d", callCount)
	}
}

func TestDiscoverSessionsWithFilters(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		q := r.URL.Query()
		if q.Get("tags__name") != "invoice" {
			t.Errorf("expected tags__name=invoice, got %q", q.Get("tags__name"))
		}
		if q.Get("correspondent__name") != "TestCorp" {
			t.Errorf("expected correspondent__name=TestCorp, got %q", q.Get("correspondent__name"))
		}
		if q.Get("created_date__gte") != "2026-06-01" {
			t.Errorf("expected created_date__gte=2026-06-01, got %q", q.Get("created_date__gte"))
		}
		_, _ = w.Write([]byte(`{"count": 0, "next": null, "results": []}`))
	}))
	defer server.Close()

	since, _ := time.Parse("2006-01-02", "2026-06-01")
	imp := NewPaperlessImporter(server.URL, "test-token", "invoice", "TestCorp", 1000)
	sessions, err := imp.DiscoverSessions(since)
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}
	if len(sessions) != 0 {
		t.Logf("got %d sessions (expected 0)", len(sessions))
	}
}

func TestImportSessionFullDocument(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`{
			"id": 42,
			"title": "Invoice March 2026",
			"content": "This is the full content of an invoice document that should be imported.",
			"created_date": "2026-03-15",
			"added": "2026-03-15T10:00:00Z",
			"correspondent": {"id": 1, "name": "Supplier GmbH"},
			"tags": [{"id": 1, "name": "invoice"}, {"id": 2, "name": "2026"}],
			"file_type": "pdf",
			"page_count": 3
		}`))
	}))
	defer server.Close()

	imp := NewPaperlessImporter(server.URL, "test-token", "", "", 200)
	ref := importer.SessionRef{SessionID: "42"}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}
	if len(facts) != 1 {
		t.Fatalf("expected 1 fact, got %d", len(facts))
	}

	fact := facts[0]
	if !strings.Contains(fact.Content, "Invoice March 2026") {
		t.Errorf("expected content to contain title, got %q", fact.Content)
	}
	if !strings.Contains(fact.Content, "Supplier GmbH") {
		t.Errorf("expected content to contain correspondent, got %q", fact.Content)
	}
	if fact.Metadata["document_id"] != "42" {
		t.Errorf("document_id = %q, want %q", fact.Metadata["document_id"], "42")
	}
	if fact.Metadata["correspondent"] != "Supplier GmbH" {
		t.Errorf("correspondent = %q, want %q", fact.Metadata["correspondent"], "Supplier GmbH")
	}
	if fact.Metadata["page_count"] != "3" {
		t.Errorf("page_count = %q, want %q", fact.Metadata["page_count"], "3")
	}
	if fact.Metadata["file_type"] != "pdf" {
		t.Errorf("file_type = %q, want %q", fact.Metadata["file_type"], "pdf")
	}
}

func TestImportSessionEmptyContent(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`{
			"id": 99,
			"title": "Empty Doc",
			"content": "",
			"created_date": "2026-01-01",
			"added": "2026-01-01T00:00:00Z",
			"correspondent": null,
			"tags": [],
			"file_type": "txt",
			"page_count": 0
		}`))
	}))
	defer server.Close()

	imp := NewPaperlessImporter(server.URL, "test-token", "", "", 100)
	ref := importer.SessionRef{SessionID: "99"}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}
	if len(facts) != 1 {
		t.Fatalf("expected 1 fact, got %d", len(facts))
	}

	if strings.Count(facts[0].Content, "\n\n") > 0 {
		t.Errorf("expected no body append for empty content, got %q", facts[0].Content)
	}
}

func TestImportSessionContentTruncation(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		longContent := ""
		for i := 0; i < 100; i++ {
			longContent += "Lorem ipsum dolor sit amet. "
		}
		_, _ = w.Write([]byte(`{
			"id": 50,
			"title": "Long Document",
			"content": "` + longContent + `",
			"created_date": "2026-01-01",
			"added": "2026-01-01T00:00:00Z",
			"correspondent": null,
			"tags": [],
			"file_type": "txt",
			"page_count": 0
		}`))
	}))
	defer server.Close()

	imp := NewPaperlessImporter(server.URL, "test-token", "", "", 50)
	ref := importer.SessionRef{SessionID: "50"}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}
	if len(facts) != 1 {
		t.Fatalf("expected 1 fact, got %d", len(facts))
	}

	if len(facts[0].Content) > 100 {
		t.Errorf("content too long after truncation: %d chars", len(facts[0].Content))
	}
	if !strings.HasSuffix(facts[0].Content, "...") {
		t.Errorf("expected truncated content to end with '...', got %q", facts[0].Content)
	}
}

func TestDoAPIRequestNon200(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusUnauthorized)
		_, _ = w.Write([]byte(`{"detail": "Invalid token"}`))
	}))
	defer server.Close()

	imp := NewPaperlessImporter(server.URL, "bad-token", "", "", 1000)
	var result interface{}
	err := imp.doAPIRequest(server.URL+"/api/documents/", &result)
	if err == nil {
		t.Fatal("expected error for 401 status")
	}
	if !strings.Contains(err.Error(), "401") {
		t.Errorf("expected error to mention 401, got %q", err.Error())
	}
}

func TestDiscoverSessionsInvalidDate(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`{
			"count": 1,
			"next": null,
			"results": [
				{
					"id": 10,
					"title": "Bad Date",
					"content": "Content.",
					"created_date": "not-a-date",
					"added": "2026-06-15T10:00:00Z",
					"correspondent": null,
					"tags": [],
					"file_type": "pdf",
					"page_count": 1
				}
			]
		}`))
	}))
	defer server.Close()

	imp := NewPaperlessImporter(server.URL, "test-token", "", "", 1000)
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}
	if len(sessions) != 1 {
		t.Fatalf("expected 1 session, got %d", len(sessions))
	}
}
