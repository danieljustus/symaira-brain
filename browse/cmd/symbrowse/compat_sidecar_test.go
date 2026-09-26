package main

import (
	"bufio"
	"bytes"
	"encoding/json"
	"fmt"
	"strings"
	"testing"

	"github.com/spf13/cobra"
)

func TestCompatSidecarRejectsUnpinnedHandshakeIdentity(t *testing.T) {
	tests := []struct {
		name      string
		component string
		oracle    string
	}{
		{name: "component", component: "other-client", oracle: "go-azuretls-v0.8.0"},
		{name: "oracle", component: "symbrowse-rust", oracle: "other-oracle"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			input := fmt.Sprintf(
				`{"type":"handshake","protocol":1,"component":%q,"oracle":%q}`+"\n",
				test.component,
				test.oracle,
			)
			command := &cobra.Command{}
			command.SetIn(strings.NewReader(input))
			var output bytes.Buffer
			command.SetOut(&output)
			if err := runCompatSidecar(command, nil); err == nil || err.Error() != "compat_identity_mismatch" {
				t.Fatalf("runCompatSidecar() error = %v, want compat_identity_mismatch", err)
			}
			if output.Len() != 0 {
				t.Fatal("invalid handshake must not acknowledge the sidecar identity")
			}
		})
	}
}

func TestCompatSidecarBoundsInboundNDJSONFrames(t *testing.T) {
	valid := strings.Repeat("x", compatMaxFrameBytes-1) + "\n"
	frame, err := readCompatFrame(bufio.NewReader(strings.NewReader(valid)))
	if err != nil || len(frame) != compatMaxFrameBytes {
		t.Fatalf("max-size frame = %d bytes, err = %v", len(frame), err)
	}

	oversized := strings.Repeat("x", compatMaxFrameBytes) + "\n"
	if _, err := readCompatFrame(bufio.NewReader(strings.NewReader(oversized))); err == nil || err.Error() != "compat_frame_too_large" {
		t.Fatalf("oversized frame error = %v, want compat_frame_too_large", err)
	}
}

func TestCompatWireKeepsRequestAndResponseHeadersDistinct(t *testing.T) {
	requestBytes, err := json.Marshal(compatRequestWire{
		Type:    "request",
		ID:      7,
		Headers: [][2]string{{"X-Request", "value"}},
	})
	if err != nil {
		t.Fatal(err)
	}
	var request compatRequestWire
	if err := json.Unmarshal(requestBytes, &request); err != nil {
		t.Fatal(err)
	}
	if len(request.Headers) != 1 || request.Headers[0] != [2]string{"X-Request", "value"} {
		t.Fatalf("request headers = %#v", request.Headers)
	}

	responseBytes, err := json.Marshal(compatResponseWire{
		Type:            "response",
		ID:              7,
		ResponseHeaders: [][2]interface{}{{"X-Response", []string{"one", "two"}}},
	})
	if err != nil {
		t.Fatal(err)
	}
	var response struct {
		Headers [][]json.RawMessage `json:"headers"`
	}
	if err := json.Unmarshal(responseBytes, &response); err != nil {
		t.Fatal(err)
	}
	if len(response.Headers) != 1 || len(response.Headers[0]) != 2 {
		t.Fatalf("response headers = %s", responseBytes)
	}
	var values []string
	if err := json.Unmarshal(response.Headers[0][1], &values); err != nil {
		t.Fatal(err)
	}
	if len(values) != 2 || values[0] != "one" || values[1] != "two" {
		t.Fatalf("response values = %#v", values)
	}
}
