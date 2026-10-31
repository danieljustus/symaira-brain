package usage

import (
	"context"
	"encoding/json"
	"errors"
	"math"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"
	"unicode"
)

const openCodeUsageURL = "https://opencode.ai/zen/go/v1/usage"

// Explicit API credentials win. An explicit cookie keeps the historical web-only
// route; neither API rejection nor unreadable native storage switches accounts.
func openCodeAPIKey() (string, string, error) {
	if os.Getenv("OPENCODE_GO_API_KEY") != "" {
		key, source, err := resolveEnv("OPENCODE_GO_API_KEY")
		if err != nil || !validOpenCodeKey(key) {
			return "", source, errors.New("OpenCode Go credential resolution failed")
		}
		return strings.TrimSpace(key), source, nil
	}
	if os.Getenv("OPENCODE_COOKIE") != "" || os.Getenv("OPENCODE_WORKSPACE_ID") != "" {
		return "", "", nil
	}
	dir := os.Getenv("OPENCODE_DATA_DIR")
	if dir == "" {
		base := os.Getenv("XDG_DATA_HOME")
		if base == "" {
			home, err := os.UserHomeDir()
			if err != nil {
				return "", "file", errors.New("OpenCode data directory unavailable")
			}
			base = filepath.Join(home, ".local", "share")
		}
		dir = filepath.Join(base, "opencode")
	}
	key, err := openCodeAuthJSONKey(dir)
	return key, "file", err
}
func validOpenCodeKey(key string) bool {
	key = strings.TrimSpace(key)
	if key == "" {
		return false
	}
	for _, c := range key {
		if unicode.IsControl(c) {
			return false
		}
	}
	return true
}
func openCodeAuthJSONKey(dir string) (string, error) {
	// A single channel is deterministic; multiple channels require an explicit key.
	entries, err := openCodeReadDir(dir)
	if os.IsNotExist(err) {
		return "", nil
	}
	if err != nil {
		return "", errors.New("OpenCode credential storage unreadable or unsupported; set OPENCODE_GO_API_KEY explicitly")
	}
	var databases []string
	for _, entry := range entries {
		if strings.HasPrefix(entry.Name(), "opencode") && strings.HasSuffix(entry.Name(), ".db") {
			databases = append(databases, entry.Name())
		}
	}
	for _, entry := range entries {
		name := entry.Name()
		if !strings.HasPrefix(name, "opencode") {
			continue
		}
		for _, suffix := range []string{"-wal", "-shm", "-journal"} {
			if strings.HasSuffix(name, ".db"+suffix) && (len(databases) != 1 || name != databases[0]+suffix) {
				return "", errors.New("OpenCode orphan database sidecar; set OPENCODE_GO_API_KEY explicitly")
			}
		}
	}
	if len(databases) > 1 {
		return "", errors.New("OpenCode database channels ambiguous; set OPENCODE_GO_API_KEY explicitly")
	}
	if len(databases) == 1 {
		key, authoritative, err := openCodeDatabaseKey(dir, databases[0])
		if err != nil || authoritative {
			return key, err
		}
	}
	path := filepath.Join(dir, "auth.json")
	data, err := openCodeReadAuthFile(path)
	if os.IsNotExist(err) {
		return "", nil
	}
	if err != nil {
		return "", errors.New("OpenCode auth.json unreadable")
	}
	var root map[string]json.RawMessage
	if json.Unmarshal(data, &root) != nil || root == nil {
		return "", errors.New("OpenCode auth.json invalid")
	}
	raw, ok := root["opencode-go"]
	if !ok {
		return "", nil
	}
	var entry struct {
		Key string `json:"key"`
	}
	if string(raw) == "null" || json.Unmarshal(raw, &entry) != nil {
		return "", errors.New("OpenCode Go credential invalid")
	}
	if entry.Key == "" {
		return "", nil
	}
	if !validOpenCodeKey(entry.Key) {
		return "", errors.New("OpenCode Go credential invalid")
	}
	return strings.TrimSpace(entry.Key), nil
}

type openCodeAPIStrategy struct {
	key    string
	client *http.Client
}

func (s *openCodeAPIStrategy) Source() string { return "api" }
func (s *openCodeAPIStrategy) Fetch(ctx context.Context) (*UsageSnapshot, error) {
	ctx, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, openCodeUsageURL, nil)
	if err != nil {
		return nil, errors.New("OpenCode Go request failed")
	}
	req.Header.Set("Authorization", "Bearer "+s.key)
	req.Header.Set("Accept", "application/json")
	resp, err := doUsageRequest(ctx, s.client, req, false)
	if err != nil {
		return nil, errors.New("OpenCode Go request failed")
	}
	data, err := readUsageBodyAndClose(resp.Body)
	if err != nil {
		return nil, errors.New("OpenCode Go response unreadable")
	}
	switch resp.StatusCode {
	case 401:
		return nil, errors.New("OpenCode Go key was rejected")
	case 403:
		return nil, errors.New("OpenCode Go subscription unavailable for this key")
	}
	if resp.StatusCode != 200 {
		return nil, errors.New("OpenCode Go usage API unavailable")
	}
	now := time.Now().UTC()
	reference := now
	if date, err := http.ParseTime(resp.Header.Get("Date")); err == nil {
		reference = date
	}
	return openCodeParseAPI(data, now, reference)
}
func openCodeParseAPI(data []byte, now, reference time.Time) (*UsageSnapshot, error) {
	bad := errors.New("OpenCode Go quota response invalid")
	var root struct {
		Usage map[string]json.RawMessage `json:"usage"`
	}
	if json.Unmarshal(data, &root) != nil || root.Usage == nil {
		return nil, bad
	}
	snap := &UsageSnapshot{ProviderID: "opencode", Source: "api", FetchedAt: now, Meters: []UsageMeter{}}
	for _, w := range []struct{ key, label string }{{"rolling", "Session"}, {"weekly", "Weekly"}, {"monthly", "Monthly"}} {
		raw, present := root.Usage[w.key]
		if !present || string(raw) == "null" {
			continue
		}
		var window struct {
			Percent  *float64   `json:"percent"`
			ResetsAt *time.Time `json:"resetsAt"`
		}
		if json.Unmarshal(raw, &window) != nil {
			return nil, bad
		}
		var used *string
		if window.Percent != nil {
			if math.IsNaN(*window.Percent) || math.IsInf(*window.Percent, 0) {
				return nil, bad
			}
			n := math.Max(0, math.Min(100, *window.Percent))
			used = strPtr(formatAmount(n))
			if w.key == "rolling" && n == 0 && window.ResetsAt != nil && math.Abs(window.ResetsAt.Sub(reference).Seconds()-18000) <= 2 {
				window.ResetsAt = nil
			}
		}
		if used != nil || window.ResetsAt != nil {
			snap.Meters = append(snap.Meters, UsageMeter{Label: w.label, Used: used, Limit: strPtr("100"), Unit: "%", ResetsAt: window.ResetsAt})
		}
	}
	if len(snap.Meters) == 0 {
		return nil, bad
	}
	return snap, nil
}
