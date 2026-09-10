package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"os"

	"github.com/danieljustus/symaira-brain/internal/broker"
	"github.com/danieljustus/symaira-brain/internal/config"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

type vaultMetadata struct {
	Path   string                 `json:"path"`
	Fields map[string]interface{} `json:"fields"`
}

func vaultBinary(stderr io.Writer, action string) (string, exitcodes.ExitCode) {
	override := ""
	if cfg, err := config.Load(); err == nil {
		override = cfg.Servers.Vault.BinaryPath
	}
	binary, err := broker.Discover("symvault", override)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain vault %s: %v\n", action, err)
		return "", exitcodes.ExitGeneric
	}
	return binary, exitcodes.ExitOK
}

func readVaultMetadata(binary, path string) (vaultMetadata, error) {
	var out bytes.Buffer
	if err := runVaultCommand(binary, []string{"get", path, "--output", "json"}, nil, &out); err != nil {
		return vaultMetadata{}, err
	}
	var detail vaultMetadata
	if err := json.Unmarshal(out.Bytes(), &detail); err != nil {
		return detail, err
	}
	return detail, nil
}

func cmdVaultSet(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	if len(args) != 1 {
		fmt.Fprintln(stderr, "symbrain vault set: usage: symbrain vault set <path> < secret.txt")
		return exitcodes.ExitNoInput
	}
	value, err := io.ReadAll(os.Stdin)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain vault set: read secret from stdin: %v\n", err)
		return exitcodes.ExitGeneric
	}
	if len(bytes.TrimSpace(value)) == 0 {
		fmt.Fprintln(stderr, "symbrain vault set: secret value from stdin is empty")
		return exitcodes.ExitNoInput
	}
	binary, code := vaultBinary(stderr, "set")
	if code != exitcodes.ExitOK {
		return code
	}
	path := args[0]
	if err := runVaultCommand(binary, []string{"set", path, "--stdin-value"}, value, nil); err != nil {
		fmt.Fprintln(stderr, "symbrain vault set: set failed")
		return exitcodes.ExitGeneric
	}
	detail, err := readVaultMetadata(binary, path)
	if err != nil {
		fmt.Fprintln(stderr, "symbrain vault set: confirmation read failed")
		return exitcodes.ExitGeneric
	}
	if detail.Path == "" {
		detail.Path = path
	}
	result := map[string]interface{}{"submitted": map[string]interface{}{"path": path}, "confirmed": map[string]interface{}{"path": detail.Path, "field_count": len(detail.Fields), "has_value": len(detail.Fields) > 0}}
	encoded, _ := json.Marshal(result)
	fmt.Fprintln(stdout, string(encoded))
	return exitcodes.ExitOK
}

func cmdVaultDelete(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	if len(args) != 2 || (args[1] != "--yes" && args[1] != "-y") {
		fmt.Fprintln(stderr, "symbrain vault delete: usage: symbrain vault delete <path> --yes")
		return exitcodes.ExitNoInput
	}
	path := args[0]
	binary, code := vaultBinary(stderr, "delete")
	if code != exitcodes.ExitOK {
		return code
	}
	if err := runVaultCommand(binary, []string{"delete", path, "--yes"}, nil, nil); err != nil {
		fmt.Fprintln(stderr, "symbrain vault delete: delete failed")
		return exitcodes.ExitGeneric
	}
	if _, err := readVaultMetadata(binary, path); err == nil {
		fmt.Fprintln(stderr, "symbrain vault delete: confirmation read found entry still present")
		return exitcodes.ExitGeneric
	}
	result := map[string]interface{}{"submitted": map[string]interface{}{"path": path}, "confirmed": map[string]interface{}{"path": path, "absent": true}}
	encoded, _ := json.Marshal(result)
	fmt.Fprintln(stdout, string(encoded))
	return exitcodes.ExitOK
}
