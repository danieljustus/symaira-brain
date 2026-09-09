package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"log/slog"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/broker"
)

type expectation struct {
	ToolNames        []string `json:"tool_names"`
	EchoText         string   `json:"echo_text"`
	ToolError        bool     `json:"tool_error"`
	ToolErrorText    string   `json:"tool_error_text"`
	TimeoutCategory  string   `json:"timeout_category"`
	MismatchCategory string   `json:"mismatch_category"`
	MismatchState    string   `json:"mismatch_state"`
	CrashSpawnCount  int      `json:"crash_spawn_count"`
	CrashState       string   `json:"crash_state"`
}

func buildFake() (string, func()) {
	dir, err := os.MkdirTemp("", "broker-oracle")
	if err != nil {
		panic(err)
	}
	path := filepath.Join(dir, "fakemcp")
	cmd := exec.Command("go", "build", "-o", path, "./internal/broker/testdata/fakemcp")
	if output, err := cmd.CombinedOutput(); err != nil {
		os.RemoveAll(dir)
		panic(fmt.Sprintf("build fakemcp: %v: %s", err, output))
	}
	return path, func() { _ = os.RemoveAll(dir) }
}

func config(path string, env []string) broker.ServerConfig {
	return broker.ServerConfig{
		Name:            "oracle",
		BinaryPath:      path,
		InitTimeout:     5 * time.Second,
		CallTimeout:     2 * time.Second,
		MaxRestarts:     1,
		BackoffBase:     10 * time.Millisecond,
		ShutdownTimeout: 500 * time.Millisecond,
		Env:             env,
		Logger:          slog.New(slog.NewTextHandler(io.Discard, nil)),
	}
}

func env(overrides ...string) []string {
	result := append([]string(nil), os.Environ()...)
	for _, override := range overrides {
		key := strings.SplitN(override, "=", 2)[0] + "="
		filtered := result[:0]
		for _, current := range result {
			if !strings.HasPrefix(current, key) {
				filtered = append(filtered, current)
			}
		}
		result = append(filtered, override)
	}
	return result
}

func generate() expectation {
	path, cleanup := buildFake()
	defer cleanup()
	ctx := context.Background()
	result := expectation{}

	basic := broker.NewManagedServer(config(path, env()))
	tools, err := basic.ListTools(ctx)
	if err != nil {
		panic(err)
	}
	for _, tool := range tools {
		result.ToolNames = append(result.ToolNames, tool.Name)
	}
	sort.Strings(result.ToolNames)
	echo, err := basic.CallTool(ctx, "echo", json.RawMessage(`{"x":1}`))
	if err != nil {
		panic(err)
	}
	result.EchoText = echo.Content[0].Text
	toolError, err := basic.CallTool(ctx, "toolerror", nil)
	if err != nil {
		panic(err)
	}
	result.ToolError = toolError.IsError
	result.ToolErrorText = toolError.Content[0].Text
	basic.Shutdown()

	timeoutCfg := config(path, env("FAKEMCP_SLOW_MS=500"))
	timeoutCfg.CallTimeout = 20 * time.Millisecond
	timeoutServer := broker.NewManagedServer(timeoutCfg)
	_, err = timeoutServer.CallTool(ctx, "slow", nil)
	var timeoutErr *broker.TimeoutError
	if errors.As(err, &timeoutErr) {
		result.TimeoutCategory = "timeout"
	}
	timeoutServer.Shutdown()

	mismatch := broker.NewManagedServer(config(path, env("FAKEMCP_PROTOCOL_VERSION=1900-01-01")))
	_, err = mismatch.ListTools(ctx)
	var mismatchErr *broker.ProtocolMismatchError
	if errors.As(err, &mismatchErr) {
		result.MismatchCategory = "protocol_mismatch"
	}
	result.MismatchState = mismatch.State().String()
	mismatch.Shutdown()

	markerDir, err := os.MkdirTemp("", "broker-spawns")
	if err != nil {
		panic(err)
	}
	defer os.RemoveAll(markerDir)
	marker := filepath.Join(markerDir, "spawns.txt")
	crash := broker.NewManagedServer(config(path, env("FAKEMCP_SPAWN_MARKER="+marker)))
	if _, err := crash.ListTools(ctx); err != nil {
		panic(err)
	}
	_, _ = crash.CallTool(ctx, "crash", nil)
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		data, _ := os.ReadFile(marker)
		if len(bytes.Fields(data)) >= 2 {
			break
		}
		time.Sleep(10 * time.Millisecond)
	}
	if _, err := crash.ListTools(ctx); err != nil {
		panic(err)
	}
	data, _ := os.ReadFile(marker)
	result.CrashSpawnCount = len(bytes.Fields(data))
	result.CrashState = crash.State().String()
	crash.Shutdown()

	return result
}

func main() {
	check := flag.Bool("check", false, "fail if generated output differs")
	output := flag.String("output", "rust/symbrain-broker/tests/fixtures/oracle_expectations.json", "output path")
	flag.Parse()
	data, err := json.MarshalIndent(generate(), "", "  ")
	if err != nil {
		panic(err)
	}
	data = append(data, '\n')
	if *check {
		existing, err := os.ReadFile(*output)
		if err != nil || !bytes.Equal(existing, data) {
			fmt.Fprintf(os.Stderr, "%s is out of date; run go run ./scripts/broker-oracle\n", *output)
			os.Exit(1)
		}
		fmt.Println("PASS: broker oracle deterministic check passed")
		return
	}
	if err := os.MkdirAll(filepath.Dir(*output), 0o755); err != nil {
		panic(err)
	}
	if err := os.WriteFile(*output, data, 0o644); err != nil {
		panic(err)
	}
	fmt.Printf("Wrote %s\n", *output)
}
