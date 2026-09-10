package usage

import (
	"context"
	"errors"
	"io"
	"net/http"
	"strings"
	"testing"
)

func TestFetchJSONRejectsUntrustedOriginBeforeTransport(t *testing.T) {
	client, transport := fakeClient(http.StatusOK, []byte(`{"ok":true}`), nil)
	req, err := http.NewRequest(http.MethodGet, "https://127.0.0.1/usage", nil)
	if err != nil {
		t.Fatal(err)
	}

	_, err = FetchJSON[map[string]bool](context.Background(), req, "test", client)
	var httpErr *HTTPError
	if !errors.As(err, &httpErr) || httpErr.Kind != "invalid_response" {
		t.Fatalf("FetchJSON error = %v, want invalid_response HTTPError", err)
	}
	if transport.lastRequest != nil {
		t.Fatal("transport received a request for an untrusted origin")
	}
}

type failingUsageBody struct {
	reader   io.Reader
	readErr  error
	closeErr error
	closed   bool
}

func (b *failingUsageBody) Read(p []byte) (int, error) {
	if b.readErr != nil {
		return 0, b.readErr
	}
	return b.reader.Read(p)
}

func (b *failingUsageBody) Close() error {
	b.closed = true
	return b.closeErr
}

func TestReadUsageBodyAndClosePreservesReadError(t *testing.T) {
	readErr := errors.New("body read failed")
	closeErr := errors.New("body close failed")
	cases := []struct {
		name       string
		body       *failingUsageBody
		wantErr    error
		wantResult string
	}{
		{
			name:       "close error after successful read",
			body:       &failingUsageBody{reader: strings.NewReader("body"), closeErr: closeErr},
			wantErr:    closeErr,
			wantResult: "",
		},
		{
			name:    "read error takes precedence",
			body:    &failingUsageBody{reader: strings.NewReader("ignored"), readErr: readErr, closeErr: closeErr},
			wantErr: readErr,
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got, err := readUsageBodyAndClose(tc.body)
			if !errors.Is(err, tc.wantErr) {
				t.Fatalf("error = %v, want %v", err, tc.wantErr)
			}
			if tc.wantResult != "" && string(got) != tc.wantResult {
				t.Fatalf("body = %q, want %q", got, tc.wantResult)
			}
			if !tc.body.closed {
				t.Fatal("body was not closed")
			}
		})
	}
}

type redirectUsageTransport struct {
	requests []*http.Request
}

func (t *redirectUsageTransport) RoundTrip(req *http.Request) (*http.Response, error) {
	t.requests = append(t.requests, req)
	return &http.Response{
		StatusCode: http.StatusFound,
		Header:     http.Header{"Location": []string{"https://evil.example.com/capture"}},
		Body:       io.NopCloser(strings.NewReader(`{"redirect":true}`)),
		Request:    req,
	}, nil
}

func TestFetchJSONRefusesRedirectWithoutForwardingCredentials(t *testing.T) {
	transport := new(redirectUsageTransport)
	client := &http.Client{
		Transport: transport,
		CheckRedirect: func(*http.Request, []*http.Request) error {
			return errors.New("redirect should not be followed")
		},
	}
	req, err := http.NewRequest(http.MethodGet, "https://api.example.com/usage", nil)
	if err != nil {
		t.Fatal(err)
	}
	req.Header.Set("Authorization", "Bearer test-token")

	_, err = FetchJSON[map[string]bool](context.Background(), req, "test", client)
	var httpErr *HTTPError
	if !errors.As(err, &httpErr) || httpErr.Kind != "status" || httpErr.Status != http.StatusFound {
		t.Fatalf("FetchJSON error = %v, want HTTP 302 status error", err)
	}
	if len(transport.requests) != 1 {
		t.Fatalf("transport requests = %d, want exactly one", len(transport.requests))
	}
	if got := transport.requests[0].Header.Get("Authorization"); got != "Bearer test-token" {
		t.Fatalf("initial Authorization = %q, want test credential", got)
	}
}
