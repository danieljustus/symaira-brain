// Command skills-yaml-oracle captures the actual shared Go frontmatter encoder.
package main

import (
	"bufio"
	"bytes"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"io"
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
	inputPath := flag.String("input", "", "JSONL inputs (default stdin)")
	checkPath := flag.String("check", "", "regenerate and compare, without rewriting")
	unicodeMode := flag.Bool("unicode", false, "emit pinned Go Unicode letter/digit ranges")
	flag.Parse()
	if *unicodeMode {
		return unicodeRanges(os.Stdout)
	}
	var input io.Reader = os.Stdin
	if *inputPath != "" {
		file, err := os.Open(*inputPath)
		if err != nil {
			return err
		}
		defer file.Close()
		input = file
	}
	var output bytes.Buffer
	if err := capture(input, &output); err != nil {
		if *checkPath == "" {
			if _, writeErr := os.Stdout.Write(output.Bytes()); writeErr != nil {
				return writeErr
			}
		}
		return err
	}
	if *checkPath != "" {
		return checkGenerated(*checkPath, output.Bytes())
	}
	_, err := os.Stdout.Write(output.Bytes())
	return err
}

func checkGenerated(path string, actual []byte) error {
	expected, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	if !bytes.Equal(expected, actual) {
		return fmt.Errorf("oracle drift: %s", path)
	}
	return nil
}

func capture(inputReader io.Reader, outputWriter io.Writer) error {
	scanner := bufio.NewScanner(inputReader)
	scanner.Buffer(make([]byte, 4096), 1024*1024)
	parseFailures := 0
	ids := map[string]bool{}
	for scanner.Scan() {
		var input struct {
			ID          string            `json:"id"`
			Frontmatter skill.Frontmatter `json:"frontmatter"`
			BodyHex     string            `json:"body_hex"`
		}
		decoder := json.NewDecoder(bytes.NewReader(scanner.Bytes()))
		decoder.UseNumber()
		if err := decoder.Decode(&input); err != nil {
			return fmt.Errorf("decode oracle input: %w", err)
		}
		var trailing any
		if err := decoder.Decode(&trailing); err != io.EOF {
			return fmt.Errorf("trailing oracle input")
		}
		if input.ID == "" || ids[input.ID] {
			return fmt.Errorf("invalid or duplicate case ID %q", input.ID)
		}
		ids[input.ID] = true
		if _, err := adapt(input.Frontmatter.Metadata); err != nil {
			return fmt.Errorf("case %s numbers: %w", input.ID, err)
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
		inputTypes, parsedTypes := map[string]string{}, map[string]string{}
		numberTypes(input.Frontmatter.Metadata, "/metadata", inputTypes)
		numberTypes(parsed.Metadata, "/metadata", parsedTypes)
		record, err := json.Marshal(map[string]any{"id": input.ID, "yaml_hex": hex.EncodeToString(output), "frontmatter": parsed, "parse_error": parseError, "body_hex": hex.EncodeToString(body), "input_number_types": inputTypes, "parsed_number_types": parsedTypes})
		if err != nil {
			return fmt.Errorf("case %s record: %w", input.ID, err)
		}
		if _, err := fmt.Fprintln(outputWriter, string(record)); err != nil {
			return fmt.Errorf("write oracle record: %w", err)
		}
	}
	if err := scanner.Err(); err != nil {
		return fmt.Errorf("read oracle input: %w", err)
	}
	if parseFailures != 0 {
		return fmt.Errorf("%d captured documents failed Go reparse", parseFailures)
	}
	if len(ids) == 0 {
		return fmt.Errorf("zero oracle cases")
	}
	return nil
}
