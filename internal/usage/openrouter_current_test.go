package usage

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"strings"
	"testing"
	"time"
)

type openRouterTestTransport func(*http.Request) (*http.Response, error)

func (f openRouterTestTransport) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }

// These examples are authored contracts, not provider captures or changes to frozen Go oracles.
func TestOpenRouterCurrentFixtures(t *testing.T) {
	var cases []struct {
		Name                 string
		Credits, Key, Legacy json.RawMessage
		CreditsStatus        int `json:"credits_status"`
		KeyStatus            int `json:"key_status"`
		Error                bool
		Meters               []UsageMeter
		Balance              string
	}
	if err := json.Unmarshal(loadFixture("openrouter-current-cases.json"), &cases); err != nil {
		t.Fatal(err)
	}
	for _, tc := range cases {
		t.Run(tc.Name, func(t *testing.T) {
			var paths []string
			client := &http.Client{Transport: openRouterTestTransport(func(r *http.Request) (*http.Response, error) {
				paths = append(paths, r.URL.Path)
				if r.URL.Scheme != "https" || r.URL.Host != "openrouter.ai" || r.Header.Get("Authorization") != "Bearer fixture-secret-must-not-escape" {
					t.Fatalf("unsafe request destination/auth")
				}
				status, body := 200, tc.Credits
				switch r.URL.Path {
				case "/api/v1/credits":
					status = tc.CreditsStatus
				case "/api/v1/key":
					status, body = tc.KeyStatus, tc.Key
				case "/api/v1/auth/key":
					body = tc.Legacy
				default:
					t.Fatalf("unexpected path: %s", r.URL.Path)
				}
				if status == -1 {
					return nil, errors.New("fixture-secret-must-not-escape")
				}
				if status == 0 {
					status = 200
				}
				return &http.Response{StatusCode: status, Body: io.NopCloser(bytes.NewReader(body)), Header: make(http.Header)}, nil
			})}
			p := &OpenRouterProvider{apiKey: "fixture-secret-must-not-escape", baseURL: openRouterDefaultBase, client: client}
			snapshot, err := p.Strategies()[0].Fetch(context.Background())
			expectedPaths := []string{"/api/v1/credits"}
			if !tc.Error {
				if tc.CreditsStatus == 404 {
					expectedPaths = append(expectedPaths, "/api/v1/auth/key")
				} else {
					expectedPaths = append(expectedPaths, "/api/v1/key")
				}
			}
			if !reflect.DeepEqual(paths, expectedPaths) {
				t.Fatalf("paths=%v want %v", paths, expectedPaths)
			}
			if tc.Error {
				if err == nil {
					t.Fatal("expected error")
				}
				if strings.Contains(err.Error(), "fixture-secret") {
					t.Fatal("secret leaked")
				}
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			if !reflect.DeepEqual(snapshot.Meters, tc.Meters) {
				actual, _ := json.Marshal(snapshot.Meters)
				want, _ := json.Marshal(tc.Meters)
				t.Fatalf("meters=%s want=%s", actual, want)
			}
			if snapshot.Balance == nil || *snapshot.Balance != tc.Balance {
				t.Fatalf("balance=%v want=%s", snapshot.Balance, tc.Balance)
			}
			if snapshot.Source != "api" || snapshot.Currency == nil || *snapshot.Currency != "USD" {
				t.Fatal("wrong source/currency")
			}
		})
	}
}

func TestCurrentFixtureCopiesMatch(t *testing.T) {
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("test source unavailable")
	}
	for _, name := range []string{"claude-current-cases.json", "openrouter-current-cases.json"} {
		goFixture, err := os.ReadFile(filepath.Join(filepath.Dir(file), "testdata", name))
		if err != nil {
			t.Fatal(err)
		}
		rustFixture, err := os.ReadFile(filepath.Join(filepath.Dir(file), "../../rust/symbrain-usage/tests/fixtures", name))
		if err != nil {
			t.Fatal(err)
		}
		if !bytes.Equal(goFixture, rustFixture) {
			t.Fatalf("fixture copy drift: %s", name)
		}
	}
}

func TestOpenRouterCurrentRejectsUnsafeDestination(t *testing.T) {
	called := false
	client := &http.Client{Transport: openRouterTestTransport(func(*http.Request) (*http.Response, error) { called = true; return nil, errors.New("must not send") })}
	p := &OpenRouterProvider{apiKey: "fixture", baseURL: "http://127.0.0.1", client: client}
	if _, err := p.Strategies()[0].Fetch(context.Background()); err == nil || called {
		t.Fatal("unsafe destination admitted")
	}
}

func TestOpenRouterCurrentPreservesCreditsNearDeadline(t *testing.T) {
	client, transport := scriptedClient(scriptedResponse{status: 200, body: []byte(`{"data":{"total_credits":10,"total_usage":1}}`)})
	ctx, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
	defer cancel()
	p := &OpenRouterProvider{apiKey: "fixture", baseURL: openRouterDefaultBase, client: client}
	snap, err := p.Strategies()[0].Fetch(ctx)
	if err != nil || snap == nil || snap.Balance == nil || *snap.Balance != "9" {
		t.Fatalf("lost required credits: %v", err)
	}
	if len(transport.requests) != 1 {
		t.Fatal("optional lookup consumed deadline")
	}
}
