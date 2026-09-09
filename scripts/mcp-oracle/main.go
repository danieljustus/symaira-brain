package main

import (
	"bytes"
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/danieljustus/symaira-corekit/mcpserver"
)

type caseDef struct {
	ID        string `json:"id"`
	Input     string `json:"input,omitempty"`
	Generator string `json:"generator,omitempty"`
}

type expectation struct {
	ID        string `json:"id"`
	Input     string `json:"input,omitempty"`
	Generator string `json:"generator,omitempty"`
	Output    string `json:"output,omitempty"`
	ErrorKind string `json:"error_kind,omitempty"`
	Error     string `json:"error,omitempty"`
}

type suite struct {
	Cases []expectation `json:"cases"`
}

const ping = `{"jsonrpc":"2.0","id":1,"method":"ping"}`
const nullPing = `{"jsonrpc":"2.0","id":null,"method":"ping"}`
const notification = `{"jsonrpc":"2.0","method":"notifications/initialized"}`
const cancellation = `{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":7,"reason":"stop"}}`

func input(tc caseDef) string {
	switch tc.Generator {
	case "oversized_line":
		return "{" + strings.Repeat(" ", 1<<20) + "\n"
	case "max_framed":
		body := ping + strings.Repeat(" ", (1<<20)-len(ping))
		return fmt.Sprintf("Content-Length: %d\r\n\r\n%s", len(body), body)
	default:
		return tc.Input
	}
}

func cases() []caseDef {
	return []caseDef{
		{ID: "line_ping", Input: ping + "\n"},
		{ID: "line_ping_eof", Input: ping},
		{ID: "line_blank_prefix", Input: "\r\n \n" + ping + "\n"},
		{ID: "line_null_id", Input: nullPing + "\n"},
		{ID: "line_notification", Input: notification + "\n"},
		{ID: "line_cancellation", Input: cancellation + "\n"},
		{ID: "framed_ping", Input: fmt.Sprintf("Content-Length: %d\r\n\r\n%s", len(ping), ping)},
		{ID: "framed_extra_header", Input: fmt.Sprintf("Content-Type: application/json\r\nContent-Length: %d\r\nX-Test: yes\r\n\r\n%s", len(ping), ping)},
		{ID: "framed_last_length_wins", Input: fmt.Sprintf("Content-Length: -1\r\nContent-Length: %d\r\n\r\n%s", len(ping), ping)},
		{ID: "framed_notification", Input: fmt.Sprintf("Content-Length: %d\r\n\r\n%s", len(notification), notification)},
		{ID: "malformed_line", Input: "{bad}\n"},
		{ID: "malformed_framed", Input: "Content-Length: 5\r\n\r\n{bad}"},
		{ID: "invalid_length_text", Input: "Content-Length: nope\r\n\r\n"},
		{ID: "invalid_length_zero", Input: "Content-Length: 0\r\n\r\n"},
		{ID: "invalid_length_negative", Input: "Content-Length: -1\r\n\r\n"},
		{ID: "invalid_length_large", Input: "Content-Length: 1048577\r\n\r\n"},
		{ID: "missing_length", Input: "Content-Type: application/json\r\n\r\n"},
		{ID: "partial_body", Input: "Content-Length: 10\r\n\r\n{}"},
		{ID: "oversized_line", Generator: "oversized_line"},
		{ID: "max_framed", Generator: "max_framed"},
	}
}

func run(tc caseDef) expectation {
	server := mcpserver.New("oracle", "1")
	var output bytes.Buffer
	err := server.ServeIO(context.Background(), strings.NewReader(input(tc)), &output)
	result := expectation{ID: tc.ID, Input: tc.Input, Generator: tc.Generator, Output: output.String()}
	if err != nil {
		result.ErrorKind = "read"
		result.Error = strings.TrimPrefix(err.Error(), "mcpserver: read error: ")
	}
	if output.Len() > 0 {
		var payload []byte
		raw := output.Bytes()
		if bytes.HasPrefix(raw, []byte("Content-Length:")) {
			parts := bytes.SplitN(raw, []byte("\r\n\r\n"), 2)
			if len(parts) == 2 {
				payload = parts[1]
			}
		} else {
			payload = bytes.TrimSpace(raw)
		}
		var response struct {
			Error *struct {
				Code int `json:"code"`
			} `json:"error"`
		}
		if json.Unmarshal(payload, &response) == nil && response.Error != nil && response.Error.Code == mcpserver.CodeParseError {
			result.ErrorKind = "parse"
		}
	}
	return result
}

func generate() suite {
	result := suite{}
	for _, tc := range cases() {
		result.Cases = append(result.Cases, run(tc))
	}
	return result
}

func syncCorpus(check bool) error {
	seeds := map[string]string{
		"fuzz/corpus/frame_decoder/line_ping":       ping + "\n",
		"fuzz/corpus/frame_decoder/framed_ping":     fmt.Sprintf("Content-Length: %d\r\n\r\n%s", len(ping), ping),
		"fuzz/corpus/jsonrpc_envelope/ping":         ping,
		"fuzz/corpus/jsonrpc_envelope/null_id":      nullPing,
		"fuzz/corpus/jsonrpc_envelope/cancellation": cancellation,
	}
	for path, content := range seeds {
		if check {
			existing, err := os.ReadFile(path)
			if err != nil || !bytes.Equal(existing, []byte(content)) {
				return fmt.Errorf("fuzz seed %s is out of date", path)
			}
			continue
		}
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			return err
		}
		if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
			return err
		}
	}
	return nil
}

func main() {
	check := flag.Bool("check", false, "fail if generated output differs")
	output := flag.String("output", "rust/symbrain-mcp/tests/fixtures/oracle_expectations.json", "output path")
	flag.Parse()
	data, err := json.MarshalIndent(generate(), "", "  ")
	if err != nil {
		panic(err)
	}
	data = append(data, '\n')
	if *check {
		existing, err := os.ReadFile(*output)
		if err != nil || !bytes.Equal(existing, data) {
			fmt.Fprintf(os.Stderr, "%s is out of date; run go run ./scripts/mcp-oracle\n", *output)
			os.Exit(1)
		}
		if err := syncCorpus(true); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		fmt.Printf("PASS: MCP oracle deterministic check passed (%d cases)\n", len(cases()))
		return
	}
	if err := os.MkdirAll(filepath.Dir(*output), 0o755); err != nil {
		panic(err)
	}
	if err := os.WriteFile(*output, data, 0o644); err != nil {
		panic(err)
	}
	if err := syncCorpus(false); err != nil {
		panic(err)
	}
	fmt.Printf("Wrote %s (%d cases)\n", *output, len(cases()))
}
