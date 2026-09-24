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

func classify(raw []byte) string {
	scanner := bufio.NewScanner(bytes.NewReader(raw))
	scanner.Buffer(make([]byte, 4096), maxFrameBytes)
	if !scanner.Scan() {
		if scanner.Err() != nil {
			return "connection_close"
		}
		return "eof"
	}
	if _, err := daemon.DecodeFrame(scanner.Bytes()); err != nil {
		return "malformed_request"
	}
	return "accepted"
}

func main() {
	prefix := `{"cmd":"x","args":{"v":"`
	suffix := `"}}`
	boundaryPayload := prefix + strings.Repeat("x", maxFrameBytes-1-len(prefix)-len(suffix)) + suffix
	oversizedPayload := prefix + strings.Repeat("x", maxFrameBytes-len(prefix)-len(suffix)) + suffix
	results := map[string]string{
		"malformed_json":    classify([]byte("{not-json}\n")),
		"empty_cmd":         classify([]byte("{}\n")),
		"boundary":          classify([]byte(boundaryPayload + "\n")),
		"oversized":         classify([]byte(oversizedPayload + "\n")),
		"vertical_tab_tail": classify([]byte("{\"cmd\":\"x\"}\x0b\n")),
		"whitespace_cmd":    classify([]byte("{\"cmd\":\" \"}\n")),
	}
	encoded, err := json.Marshal(results)
	if err != nil {
		panic(err)
	}
	fmt.Println(string(encoded))
}
