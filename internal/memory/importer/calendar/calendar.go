package calendar

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
	"golang.org/x/oauth2"
)

// CalendarImporter imports events from Google Calendar.
type CalendarImporter struct {
	calendarID  string // "primary" or specific calendar ID
	includeDesc bool
	tokenPath   string // path to OAuth token JSON
	includeDays int    // upcoming days to include

	// vaultLookup is the optional SymVault fallback used when no token file
	// or env var is configured. It is an injectable seam for tests; the
	// default runs `symvault get <path> --print` under a bounded context.
	vaultLookup func(ctx context.Context, path string) ([]byte, error)

	// vaultTimeout bounds vaultLookup so a missing or hung symvault cannot
	// stall the no-token path. Defaults to vaultLookupTimeout.
	vaultTimeout time.Duration
}

// calendarEvent represents a Google Calendar event from the API.
type calendarEvent struct {
	ID          string            `json:"id"`
	Summary     string            `json:"summary"`
	Description string            `json:"description"`
	Location    string            `json:"location"`
	StartTime   eventDateTime     `json:"start"`
	EndTime     eventDateTime     `json:"end"`
	Attendees   []eventAttendee   `json:"attendees"`
	Organizer   eventOrganizer    `json:"organizer"`
	Status      string            `json:"status"`
	Created     string            `json:"created"`
	Updated     string            `json:"updated"`
	RecurringID string            `json:"recurringEventId"`
	HTMLLink    string            `json:"htmlLink"`
	Metadata    map[string]string `json:"conferenceData"` // not used but present
}

type eventDateTime struct {
	DateTime string `json:"dateTime"`
	Date     string `json:"date"` // all-day events
	TimeZone string `json:"timeZone"`
}

type eventAttendee struct {
	Email  string `json:"email"`
	Name   string `json:"name"`
	Status string `json:"responseStatus"` // "accepted", "declined", "tentative", "needsAction"
}

type eventOrganizer struct {
	Email string `json:"email"`
	Name  string `json:"name"`
}

// calendarListResponse represents the Google Calendar API response.
type calendarListResponse struct {
	Items         []calendarEvent `json:"items"`
	NextPageToken string          `json:"nextPageToken"`
}

// NewCalendarImporter creates a new calendar importer.
func NewCalendarImporter(calendarID, tokenPath string, includeDesc bool, includeDays int) *CalendarImporter {
	if calendarID == "" {
		calendarID = "primary"
	}
	if includeDays <= 0 {
		includeDays = 7
	}
	return &CalendarImporter{
		calendarID:   calendarID,
		includeDesc:  includeDesc,
		tokenPath:    tokenPath,
		includeDays:  includeDays,
		vaultLookup:  defaultVaultLookup,
		vaultTimeout: vaultLookupTimeout,
	}
}

func (c *CalendarImporter) Name() string { return "calendar" }

func (c *CalendarImporter) Category() string { return "calendar" }

func (c *CalendarImporter) PrivacyLevel() importer.PrivacyLevel {
	return importer.PrivacyConfidential
}

func (c *CalendarImporter) RequiresPIIGuard() bool { return true }

// DiscoverSessions finds calendar events since the given time.
func (c *CalendarImporter) DiscoverSessions(since time.Time) ([]importer.SessionRef, error) {
	token, err := c.loadToken()
	if err != nil {
		return nil, fmt.Errorf("failed to load OAuth token: %w", err)
	}

	events, err := c.fetchEvents(since, token)
	if err != nil {
		return nil, fmt.Errorf("failed to fetch events: %w", err)
	}

	var sessions []importer.SessionRef
	for _, event := range events {
		if event.Status == "cancelled" {
			continue
		}

		startTime := c.parseEventTime(event.StartTime)
		if startTime.IsZero() {
			continue
		}

		session := importer.SessionRef{
			Tool:       "calendar",
			SessionID:  event.ID,
			Path:       fmt.Sprintf("google-calendar://%s/%s", c.calendarID, event.ID),
			ModifiedAt: startTime,
			Metadata: map[string]string{
				"title":    event.Summary,
				"location": event.Location,
			},
		}

		if event.Organizer.Name != "" {
			session.Metadata["organizer"] = event.Organizer.Name
		} else if event.Organizer.Email != "" {
			session.Metadata["organizer"] = event.Organizer.Email
		}

		if len(event.Attendees) > 0 {
			var names []string
			for _, a := range event.Attendees {
				if a.Name != "" {
					names = append(names, a.Name)
				} else {
					names = append(names, a.Email)
				}
			}
			session.Metadata["attendees"] = strings.Join(names, ", ")
		}

		if event.RecurringID != "" {
			session.Metadata["recurring"] = "true"
		}

		sessions = append(sessions, session)
	}

	return sessions, nil
}

// ImportSession imports a single calendar event as facts.
func (c *CalendarImporter) ImportSession(ref importer.SessionRef) ([]importer.ImportedFact, error) {
	token, err := c.loadToken()
	if err != nil {
		return nil, fmt.Errorf("failed to load OAuth token: %w", err)
	}

	event, err := c.fetchEvent(ref.SessionID, token)
	if err != nil {
		return nil, fmt.Errorf("failed to fetch event: %w", err)
	}

	startTime := c.parseEventTime(event.StartTime)
	endTime := c.parseEventTime(event.EndTime)

	// Build content
	content := fmt.Sprintf("Calendar event: %s", event.Summary)

	if !startTime.IsZero() {
		content += fmt.Sprintf(" on %s", startTime.Format("2006-01-02 15:04"))
	}

	if event.Location != "" {
		content += fmt.Sprintf(" at %s", event.Location)
	}

	if c.includeDesc && event.Description != "" {
		desc := event.Description
		if len(desc) > 2000 {
			desc = desc[:2000] + "..."
		}
		content += "\n\n" + desc
	}

	metadata := map[string]string{
		"event_id": event.ID,
		"title":    event.Summary,
		"source":   "calendar",
		"calendar": c.calendarID,
	}

	if !startTime.IsZero() {
		metadata["start"] = startTime.Format(time.RFC3339)
	}
	if !endTime.IsZero() {
		metadata["end"] = endTime.Format(time.RFC3339)
	}

	if event.Location != "" {
		metadata["location"] = event.Location
	}

	if event.Organizer.Email != "" {
		metadata["organizer"] = event.Organizer.Email
	}

	if len(event.Attendees) > 0 {
		var emails []string
		for _, a := range event.Attendees {
			emails = append(emails, a.Email)
		}
		jsonEmails, _ := json.Marshal(emails)
		metadata["attendees"] = string(jsonEmails)
	}

	if event.RecurringID != "" {
		metadata["recurring"] = "true"
		metadata["recurring_id"] = event.RecurringID
	}

	return []importer.ImportedFact{{
		Content:   content,
		Source:    "calendar",
		SessionID: ref.SessionID,
		Timestamp: startTime,
		Metadata:  metadata,
	}}, nil
}

// vaultLookupTimeout bounds the optional SymVault fallback used when no
// token file or env var is configured, so a missing or hung symvault cannot
// stall discovery/import.
const vaultLookupTimeout = 3 * time.Second

// defaultVaultLookup runs `symvault get <path> --print` and returns its raw
// stdout. The context is bounded by the caller so a hung symvault cannot
// block indefinitely.
func defaultVaultLookup(ctx context.Context, path string) ([]byte, error) {
	return exec.CommandContext(ctx, "symvault", "get", path, "--print").Output()
}

// loadToken loads the OAuth2 token from file or environment, falling back to
// a bounded SymVault lookup when neither is configured.
func (c *CalendarImporter) loadToken() (*oauth2.Token, error) {
	tokenPath := c.tokenPath
	if tokenPath == "" {
		tokenPath = os.Getenv("GOOGLE_CALENDAR_TOKEN")
	}

	if tokenPath == "" {
		// Optional SymVault fallback, bounded so a missing or hung symvault
		// cannot stall the no-token path for tens of seconds.
		lookup := c.vaultLookup
		if lookup == nil {
			lookup = defaultVaultLookup
		}
		timeout := c.vaultTimeout
		if timeout <= 0 {
			timeout = vaultLookupTimeout
		}
		ctx, cancel := context.WithTimeout(context.Background(), timeout)
		defer cancel()
		out, err := lookup(ctx, "google/calendar-token")
		if err == nil {
			tokenData := strings.TrimSpace(string(out))
			return parseToken([]byte(tokenData))
		}
		return nil, fmt.Errorf("no token found: set GOOGLE_CALENDAR_TOKEN env or provide --token-path")
	}

	data, err := os.ReadFile(tokenPath)
	if err != nil {
		return nil, fmt.Errorf("failed to read token file: %w", err)
	}

	return parseToken(data)
}

func parseToken(data []byte) (*oauth2.Token, error) {
	var token oauth2.Token
	if err := json.Unmarshal(data, &token); err != nil {
		return nil, fmt.Errorf("failed to parse token JSON: %w", err)
	}
	return &token, nil
}

// fetchEvents fetches events from the Google Calendar API.
func (c *CalendarImporter) fetchEvents(since time.Time, token *oauth2.Token) ([]calendarEvent, error) {
	timeMin := since.Format(time.RFC3339)
	timeMax := time.Now().AddDate(0, 0, c.includeDays).Format(time.RFC3339)

	url := fmt.Sprintf(
		"https://www.googleapis.com/calendar/v3/calendars/%s/events?timeMin=%s&timeMax=%s&singleEvents=true&orderBy=startTime&maxResults=250",
		c.calendarID, timeMin, timeMax,
	)

	var allEvents []calendarEvent
	for url != "" {
		var resp calendarListResponse
		if err := c.doAPIRequest(url, token, &resp); err != nil {
			return nil, err
		}
		allEvents = append(allEvents, resp.Items...)

		if resp.NextPageToken != "" {
			url = fmt.Sprintf("%s&pageToken=%s", url, resp.NextPageToken)
		} else {
			break
		}
	}

	return allEvents, nil
}

// fetchEvent fetches a single event by ID.
func (c *CalendarImporter) fetchEvent(eventID string, token *oauth2.Token) (*calendarEvent, error) {
	url := fmt.Sprintf(
		"https://www.googleapis.com/calendar/v3/calendars/%s/events/%s",
		c.calendarID, eventID,
	)

	var event calendarEvent
	if err := c.doAPIRequest(url, token, &event); err != nil {
		return nil, err
	}

	return &event, nil
}

// doAPIRequest performs an authenticated HTTP request to the Google Calendar API.
func (c *CalendarImporter) doAPIRequest(url string, token *oauth2.Token, result interface{}) error {
	ctx := context.Background()
	client := oauth2.NewClient(ctx, oauth2.StaticTokenSource(token))

	resp, err := client.Get(url)
	if err != nil {
		return fmt.Errorf("API request failed: %w", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != 200 {
		return fmt.Errorf("API returned status %d", resp.StatusCode)
	}

	return json.NewDecoder(resp.Body).Decode(result)
}

// parseEventTime extracts time.Time from a calendarEvent's eventDateTime.
func (c *CalendarImporter) parseEventTime(dt eventDateTime) time.Time {
	if dt.DateTime != "" {
		t, _ := time.Parse(time.RFC3339, dt.DateTime)
		return t
	}
	if dt.Date != "" {
		t, _ := time.Parse("2006-01-02", dt.Date)
		return t
	}
	return time.Time{}
}
