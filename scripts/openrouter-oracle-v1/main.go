// Command openrouter-oracle-v1 records offline production Go reports, never provider traffic.
package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/usage"
)

type reply struct {
	Endpoint string            `json:"endpoint"`
	Status   int               `json:"status"`
	Body     string            `json:"body"`
	Headers  map[string]string `json:"headers"`
}
type request struct {
	Method  string              `json:"method"`
	URL     string              `json:"url"`
	Headers map[string][]string `json:"headers"`
	Body    string              `json:"body"`
}
type input struct {
	ID      string  `json:"id"`
	Base    string  `json:"base"`
	Replies []reply `json:"replies"`
}
type result struct {
	Input    input        `json:"input"`
	Requests []request    `json:"requests"`
	Report   usage.Report `json:"report"`
}
type recorder struct {
	replies  []reply
	requests []request
}

func (r *recorder) RoundTrip(req *http.Request) (*http.Response, error) {
	body := ""
	if req.Body != nil {
		data, err := io.ReadAll(req.Body)
		if err != nil {
			return nil, err
		}
		body = string(data)
	}
	headers := map[string][]string{}
	for name, values := range req.Header {
		headers[strings.ToLower(name)] = values
	}
	r.requests = append(r.requests, request{req.Method, req.URL.String(), headers, body})
	if len(r.replies) == 0 {
		panic("unexpected extra request")
	}
	reply := r.replies[0]
	r.replies = r.replies[1:]
	if !strings.HasSuffix(req.URL.Path, reply.Endpoint) {
		panic("unexpected endpoint")
	}
	if reply.Status == -1 {
		return nil, errors.New("oracle-only-invalid-transport-detail")
	}
	h := http.Header{}
	for name, value := range reply.Headers {
		h.Set(name, value)
	}
	return &http.Response{StatusCode: reply.Status, Body: io.NopCloser(strings.NewReader(reply.Body)), Header: h, Request: req}, nil
}
func run() error {
	var inputs []input
	if err := json.NewDecoder(os.Stdin).Decode(&inputs); err != nil {
		return err
	}
	if err := os.Setenv("OPENROUTER_API_KEY", "oracle-only-invalid-v1-credential"); err != nil {
		return err
	}
	results := []result{}
	ids := map[string]bool{}
	for _, in := range inputs {
		if in.ID == "" || ids[in.ID] {
			return fmt.Errorf("missing or duplicate case ID")
		}
		ids[in.ID] = true
		if err := os.Setenv("OPENROUTER_API_URL", in.Base); err != nil {
			return err
		}
		transport := &recorder{replies: in.Replies, requests: []request{}}
		provider := usage.NewOpenRouterProvider(&http.Client{Transport: transport})
		report := usage.BuildReport(context.Background(), []usage.Provider{provider})
		if len(transport.replies) != 0 {
			return fmt.Errorf("%s: unconsumed replies", in.ID)
		}
		// The sole normalization is the nondeterministic acquisition timestamp.
		if snap := report.Providers[0].Snapshot; snap != nil {
			snap.FetchedAt = time.Unix(0, 0).UTC()
		}
		results = append(results, result{in, transport.requests, report})
	}
	return json.NewEncoder(os.Stdout).Encode(struct {
		Version int      `json:"schema_version"`
		Cases   []result `json:"cases"`
	}{1, results})
}
func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
