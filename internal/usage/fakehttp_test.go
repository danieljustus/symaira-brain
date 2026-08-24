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

func loadFixture(name string) []byte {
	data, err := os.ReadFile("testdata/" + name)
	if err != nil {
		panic(err) // test fixture must exist — a missing file is a test bug
	}
	return data
}

var errBoom = errors.New("boom")
