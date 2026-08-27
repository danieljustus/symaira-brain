package syncclient

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

// defaultTimeout bounds a whole sync run when no explicit timeout is set.
const defaultTimeout = 60 * time.Second

// defaultPageLimit is the per-request page size for pulls.
const defaultPageLimit = 500

// maxResponseBody is the per-response body limit for the sync client.
// 10 MiB keeps memory use bounded while allowing large sync payloads.
const maxResponseBody = 10 << 20

// HTTPError reports a non-2xx response from the remote memory server.
type HTTPError struct {
	Status  int
	Code    string
	Message string
}

func (e *HTTPError) Error() string {
	if e.Code != "" {
		return fmt.Sprintf("remote returned %s (%d): %s", e.Code, e.Status, e.Message)
	}
	return fmt.Sprintf("remote returned status %d: %s", e.Status, e.Message)
}

// ChangesResponse mirrors GET /api/sync/changes on the remote server.
type ChangesResponse struct {
	Memories   []*db.Memory       `json:"memories"`
	Deleted    []db.DeletedMemory `json:"deleted"`
	ServerTime time.Time          `json:"server_time"`
	NextCursor string             `json:"next_cursor,omitempty"`
}

// ApplyResult mirrors POST /api/sync/apply on the remote server.
type ApplyResult struct {
	Applied             int `json:"applied"`
	Skipped             int `json:"skipped"`
	Deleted             int `json:"deleted"`
	SkippedInvalidScope int `json:"skippedInvalidScope"`
	SkippedInvalidID    int `json:"skippedInvalidID"`
}

// RelayResponse mirrors GET /api/sync/relay on the remote server.
type RelayResponse struct {
	Blobs      []db.RelayBlob `json:"blobs"`
	ServerTime time.Time      `json:"server_time"`
}

// RelayPushResult mirrors POST /api/sync/relay on the remote server.
type RelayPushResult struct {
	Stored  int `json:"stored"`
	Skipped int `json:"skipped"`
}

// ErrInsecureRemote is returned by ValidateRemoteURL when the remote URL
// uses plain HTTP and neither targets a loopback host nor is allowed by
// the explicit override.
var ErrInsecureRemote = errors.New("remote memory sync requires an https URL")

// Client is a minimal HTTP client for the memory sync API. It is safe to
// reuse across runs; the token is sent on every request when non-empty.
// Construction validates the base URL: HTTPS is required, with a
// loopback-HTTP escape hatch (see ValidateRemoteURL).
type Client struct {
	baseURL string
	token   string
	http    *http.Client
}

// NewClient returns a client for the given remote base URL. A nil http
// client yields a default one with defaultTimeout. The token is optional;
// empty disables the Authorization header.
//
// The remote URL is validated synchronously: it must parse, carry an http
// or https scheme, and either use https or target a loopback host. Pass
// NewClientWithOptions(..., allowInsecure=true) to override this guard
// and ship bearer tokens over plain HTTP anyway; callers that do so are
// responsible for the warning.
func NewClient(baseURL, token string, hc *http.Client) (*Client, error) {
	return NewClientWithOptions(baseURL, token, hc, false)
}

// NewClientWithOptions is NewClient with explicit control over the
// insecure-HTTP override. When allowInsecure is true the caller is opting
// into sending bearer tokens over plain HTTP and is responsible for
// surfacing the warning to the user.
func NewClientWithOptions(baseURL, token string, hc *http.Client, allowInsecure bool) (*Client, error) {
	if err := ValidateRemoteURL(baseURL, allowInsecure); err != nil {
		return nil, err
	}
	if hc == nil {
		hc = &http.Client{Timeout: defaultTimeout}
	}
	return &Client{baseURL: strings.TrimRight(baseURL, "/"), token: token, http: hc}, nil
}

// ValidateRemoteURL checks that raw is a well-formed http(s) URL and that
// it uses https, except for loopback hosts which are allowed over plain
// HTTP for local development. When allowInsecure is true the https
// requirement is dropped; callers that exercise this override should log
// a clear warning alongside it.
func ValidateRemoteURL(raw string, allowInsecure bool) error {
	if raw == "" {
		return errors.New("remote URL is required")
	}
	u, err := url.Parse(raw)
	if err != nil {
		return fmt.Errorf("parse remote URL: %w", err)
	}
	switch strings.ToLower(u.Scheme) {
	case "https":
		return nil
	case "http":
		host := u.Hostname()
		if host == "" {
			return fmt.Errorf("%w: http URL %q has no host", ErrInsecureRemote, raw)
		}
		if isLoopbackHost(host) {
			return nil
		}
		if allowInsecure {
			return nil
		}
		return fmt.Errorf("%w: %s (pass --allow-insecure-http to override)", ErrInsecureRemote, raw)
	default:
		if u.Scheme == "" {
			return fmt.Errorf("parse remote URL: missing scheme in %q", raw)
		}
		return fmt.Errorf("remote URL scheme %q is not supported (use https, or http for loopback)", u.Scheme)
	}
}

// isLoopbackHost reports whether host is a loopback name or IP. The
// comparison is case-insensitive for host names and exact for IPs.
func isLoopbackHost(host string) bool {
	if strings.EqualFold(host, "localhost") {
		return true
	}
	ip := net.ParseIP(host)
	if ip == nil {
		return false
	}
	return ip.IsLoopback()
}

// Changes fetches remote changes. When cursor is non-empty it is passed as
// the pagination cursor and takes precedence over since; otherwise since
// selects everything strictly after the given time (zero time = everything).
func (c *Client) Changes(ctx context.Context, since time.Time, cursor string, limit int) (*ChangesResponse, error) {
	u := c.baseURL + "/api/sync/changes"
	q := url.Values{}
	if cursor != "" {
		q.Set("cursor", cursor)
	} else if !since.IsZero() {
		q.Set("since", since.UTC().Format(time.RFC3339))
	}
	if limit > 0 {
		q.Set("limit", strconv.Itoa(limit))
	}
	if len(q) > 0 {
		u += "?" + q.Encode()
	}
	var out ChangesResponse
	if err := c.do(ctx, http.MethodGet, u, nil, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Apply pushes local memories and tombstones to the remote server and
// returns the server's per-operation counters.
func (c *Client) Apply(ctx context.Context, memories []*db.Memory, deleted []db.DeletedMemory) (*ApplyResult, error) {
	body := struct {
		Memories []*db.Memory       `json:"memories"`
		Deleted  []db.DeletedMemory `json:"deleted"`
	}{Memories: memories, Deleted: deleted}
	var out ApplyResult
	if err := c.do(ctx, http.MethodPost, c.baseURL+"/api/sync/apply", body, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// RelayPull fetches encrypted relay blobs updated strictly after since.
func (c *Client) RelayPull(ctx context.Context, since time.Time, limit int) (*RelayResponse, error) {
	u := c.baseURL + "/api/sync/relay"
	q := url.Values{}
	if !since.IsZero() {
		q.Set("since", since.UTC().Format(time.RFC3339Nano))
	}
	if limit > 0 {
		q.Set("limit", strconv.Itoa(limit))
	}
	if len(q) > 0 {
		u += "?" + q.Encode()
	}
	var out RelayResponse
	if err := c.do(ctx, http.MethodGet, u, nil, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// RelayPush stores encrypted relay blobs on the remote server.
func (c *Client) RelayPush(ctx context.Context, blobs []db.RelayBlob) (*RelayPushResult, error) {
	body := struct {
		Blobs []db.RelayBlob `json:"blobs"`
	}{Blobs: blobs}
	var out RelayPushResult
	if err := c.do(ctx, http.MethodPost, c.baseURL+"/api/sync/relay", body, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// do performs one request, sets the bearer token, decodes JSON into out and
// converts non-2xx responses into *HTTPError.
func (c *Client) do(ctx context.Context, method, target string, body any, out any) error {
	var r io.Reader
	if body != nil {
		buf, err := json.Marshal(body)
		if err != nil {
			return fmt.Errorf("encode request body: %w", err)
		}
		r = bytes.NewReader(buf)
	}
	req, err := http.NewRequestWithContext(ctx, method, target, r)
	if err != nil {
		return fmt.Errorf("build request: %w", err)
	}
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	if c.token != "" {
		req.Header.Set("Authorization", "Bearer "+c.token)
	}

	resp, err := c.http.Do(req)
	if err != nil {
		return fmt.Errorf("request %s: %w", c.baseURL, err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode < http.StatusOK || resp.StatusCode > http.StatusMultipleChoices {
		var apiErr struct {
			Error string `json:"error"`
			Code  string `json:"code"`
		}
		_ = json.NewDecoder(io.LimitReader(resp.Body, 1<<16)).Decode(&apiErr)
		return &HTTPError{Status: resp.StatusCode, Code: apiErr.Code, Message: apiErr.Error}
	}
	if out != nil {
		limited := io.LimitReader(resp.Body, maxResponseBody)
		if err := json.NewDecoder(limited).Decode(out); err != nil {
			return fmt.Errorf("decode remote response: %w", err)
		}
	}
	return nil
}
