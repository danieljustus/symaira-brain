package usage

import (
	"bytes"
	"errors"
	"io"
	"net/http"
	"os"
)

// fakeTransport is a scripted http.RoundTripper: returns a canned
// status/body/headers, or a canned error, and records the last request it
// saw. Mirrors symaira-cockpit's FakeNetwork test seam.
type fakeTransport struct {
	status  int
	body    []byte
	headers map[string]string
	err     error

	lastRequest *http.Request
}

func (t *fakeTransport) RoundTrip(req *http.Request) (*http.Response, error) {
	t.lastRequest = req
	if t.err != nil {
		return nil, t.err
	}
	resp := &http.Response{
		StatusCode: t.status,
		Body:       io.NopCloser(bytes.NewReader(t.body)),
		Header:     make(http.Header),
	}
	for k, v := range t.headers {
		resp.Header.Set(k, v)
	}
	return resp, nil
}

func fakeClient(status int, body []byte, headers map[string]string) (*http.Client, *fakeTransport) {
	transport := &fakeTransport{status: status, body: body, headers: headers}
	return &http.Client{Transport: transport}, transport
}

func fakeErrorClient(err error) (*http.Client, *fakeTransport) {
	transport := &fakeTransport{err: err}
	return &http.Client{Transport: transport}, transport
}

// scriptedTransport returns one canned response per request, in order, and
// records every request it saw. Mirrors symaira-cockpit's ScriptedNetwork
// test seam (used where a strategy makes more than one HTTP call).
type scriptedTransport struct {
	responses []scriptedResponse
	requests  []*http.Request
}

type scriptedResponse struct {
	status int
	body   []byte
}

func (t *scriptedTransport) RoundTrip(req *http.Request) (*http.Response, error) {
	t.requests = append(t.requests, req)
	if len(t.requests) > len(t.responses) {
		return nil, errBoom
	}
	r := t.responses[len(t.requests)-1]
	return &http.Response{
		StatusCode: r.status,
		Body:       io.NopCloser(bytes.NewReader(r.body)),
		Header:     make(http.Header),
	}, nil
}

func scriptedClient(responses ...scriptedResponse) (*http.Client, *scriptedTransport) {
	transport := &scriptedTransport{responses: responses}
	return &http.Client{Transport: transport}, transport
}

func loadFixture(name string) []byte {
	data, err := os.ReadFile("testdata/" + name)
	if err != nil {
		panic(err) // test fixture must exist — a missing file is a test bug
	}
	return data
}

var errBoom = errors.New("boom")
