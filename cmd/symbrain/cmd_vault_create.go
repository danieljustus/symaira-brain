package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"os/exec"
	"time"

	"github.com/danieljustus/symaira-brain/internal/broker"
	"github.com/danieljustus/symaira-brain/internal/config"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// cmdVaultCreate creates one entry through symvault. The secret is read only
// from stdin and is never included in argv or output.
func cmdVaultCreate(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	if len(args) != 1 {
		fmt.Fprintln(stderr, "symbrain vault create: usage: symbrain vault create <path> < secret.txt")
		return exitcodes.ExitNoInput
	}
	path := args[0]
	if !validVaultPath(path) {
		fmt.Fprintln(stderr, "symbrain vault create: usage: symbrain vault create <path> < secret.txt")
		return exitcodes.ExitNoInput
	}
	value, err := readVaultSecretStdin()
	if err != nil {
		fmt.Fprintf(stderr, "symbrain vault create: %v\n", err)
		return exitcodes.ExitNoInput
	}

	override := ""
	if cfg, loadErr := config.Load(); loadErr == nil {
		override = cfg.Servers.Vault.BinaryPath
	}
	binary, err := broker.Discover("symvault", override)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain vault create: %v\n", err)
		return exitcodes.ExitGeneric
	}

	if err := runVaultCommand(binary, []string{"add", path, "--stdin-value"}, append(value, '\n'), nil); err != nil {
		fmt.Fprintf(stderr, "symbrain vault create: create failed\n")
		return exitcodes.ExitGeneric
	}
	var getOutput bytes.Buffer
	if err := runVaultCommand(binary, []string{"get", path, "--output", "json"}, nil, &getOutput); err != nil {
		fmt.Fprintf(stderr, "symbrain vault create: confirmation read failed\n")
		return exitcodes.ExitGeneric
	}
	var detail struct {
		Path   string                 `json:"path"`
		Fields map[string]interface{} `json:"fields"`
	}
	if err := json.Unmarshal(getOutput.Bytes(), &detail); err != nil {
		fmt.Fprintln(stderr, "symbrain vault create: confirmation was not valid JSON")
		return exitcodes.ExitGeneric
	}
	if detail.Path == "" {
		detail.Path = path
	}
	result := map[string]interface{}{
		"submitted": map[string]interface{}{"path": path},
		"confirmed": map[string]interface{}{
			"path": detail.Path, "field_count": len(detail.Fields), "has_value": len(detail.Fields) > 0,
		},
	}
	encoded, _ := json.Marshal(result)
	fmt.Fprintln(stdout, string(encoded))
	return exitcodes.ExitOK
}

func runVaultCommand(binary string, args []string, stdin []byte, stdout io.Writer) error {
	ctx, cancel := context.WithTimeout(context.Background(), time.Minute)
	defer cancel()
	cmd := exec.CommandContext(ctx, binary, args...)
	cmd.Stdin = bytes.NewReader(stdin)
	var stderr bytes.Buffer
	cmd.Stderr = &stderr
	if stdout == nil {
		stdout = io.Discard
	}
	cmd.Stdout = stdout
	if err := cmd.Run(); err != nil {
		return err
	}
	return nil
}
