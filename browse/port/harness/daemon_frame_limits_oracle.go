//go:build ignore

// Command daemon_frame_limits_oracle exercises Go's production frame decoder
// behind the same bufio.Scanner line framing used by internal/daemon.Server.
package main

import (
	"bufio"
	"bytes"
	"encoding/json"
	"fmt"
	"strings"

	"github.com/danieljustus/symaira-browse/internal/daemon"
)

const maxFrameBytes = 1 << 20

type result struct {
	Class   string `json:"class"`
	Message string `json:"message"`
}

func classify(raw []byte) result {
	scanner := bufio.NewScanner(bytes.NewReader(raw))
	scanner.Buffer(make([]byte, 4096), maxFrameBytes)
	if !scanner.Scan() {
		if scanner.Err() != nil {
			return result{Class: "connection_close"}
		}
		return result{Class: "eof"}
	}
	if _, err := daemon.DecodeFrame(scanner.Bytes()); err != nil {
		return result{Class: "malformed_request", Message: err.Error()}
	}
	return result{Class: "accepted"}
}

func main() {
	prefix := `{"cmd":"x","args":{"v":"`
	suffix := `"}}`
	boundaryPayload := prefix + strings.Repeat("x", maxFrameBytes-1-len(prefix)-len(suffix)) + suffix
	oversizedPayload := prefix + strings.Repeat("x", maxFrameBytes-len(prefix)-len(suffix)) + suffix
	results := map[string]result{
		"malformed_json":      classify([]byte("{not json\n")),
		"truncated_object":    classify([]byte("{\"cmd\":\n")),
		"truncated_string":    classify([]byte("{\"cmd\":\"x\"\n")),
		"invalid_escape":      classify([]byte("{\"cmd\":\"x\\q\"}\n")),
		"leading_zero_number": classify([]byte("{\"cmd\":01}\n")),
		"invalid_exponent":    classify([]byte("{\"cmd\":1e}\n")),
		"empty_cmd":           classify([]byte("{}\n")),
		"boundary":            classify([]byte(boundaryPayload + "\n")),
		"oversized":           classify([]byte(oversizedPayload + "\n")),
		"vertical_tab_tail":   classify([]byte("{\"cmd\":\"x\"}\x0b\n")),
		"whitespace_cmd":      classify([]byte("{\"cmd\":\" \"}\n")),
	}
	encoded, err := json.Marshal(results)
	if err != nil {
		panic(err)
	}
	fmt.Println(string(encoded))
}
