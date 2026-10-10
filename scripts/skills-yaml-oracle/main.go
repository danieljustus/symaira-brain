// Command skills-yaml-oracle captures the actual shared Go frontmatter encoder.
package main

import (
	"bufio"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"

	"github.com/danieljustus/symaira-brain/internal/skills/skill"
	"gopkg.in/yaml.v3"
)

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func run() error {
	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 4096), 1024*1024)
	parseFailures := 0
	for scanner.Scan() {
		var input struct {
			ID          string            `json:"id"`
			Frontmatter skill.Frontmatter `json:"frontmatter"`
			BodyHex     string            `json:"body_hex"`
		}
		if err := json.Unmarshal(scanner.Bytes(), &input); err != nil {
			return fmt.Errorf("decode oracle input: %w", err)
		}
		body, err := hex.DecodeString(input.BodyHex)
		if err != nil {
			return fmt.Errorf("case %s body: %w", input.ID, err)
		}
		header, err := yaml.Marshal(input.Frontmatter)
		if err != nil {
			return fmt.Errorf("case %s encode: %w", input.ID, err)
		}
		output := append([]byte("---\n"), header...)
		output = append(output, []byte("---\n\n")...)
		output = append(output, body...)
		var parsed skill.Frontmatter
		parseError := ""
		if err := yaml.Unmarshal(header, &parsed); err != nil {
			// Retain invalid reference-domain output instead of aborting capture.
			parseError = err.Error()
			parseFailures++
		}
		record, err := json.Marshal(map[string]any{"id": input.ID, "yaml_hex": hex.EncodeToString(output), "frontmatter": parsed, "parse_error": parseError, "body_hex": hex.EncodeToString(body)})
		if err != nil {
			return fmt.Errorf("case %s record: %w", input.ID, err)
		}
		if _, err := fmt.Fprintln(os.Stdout, string(record)); err != nil {
			return fmt.Errorf("write oracle record: %w", err)
		}
	}
	if err := scanner.Err(); err != nil {
		return fmt.Errorf("read oracle input: %w", err)
	}
	if parseFailures != 0 {
		return fmt.Errorf("%d captured documents failed Go reparse", parseFailures)
	}
	return nil
}
