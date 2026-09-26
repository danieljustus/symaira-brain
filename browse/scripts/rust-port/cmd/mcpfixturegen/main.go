// Command mcpfixturegen records raw MCP stdio frames from the Go server while
// proxying tool calls through an isolated, deterministic daemon fixture.
package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"sort"
	"time"

	"github.com/danieljustus/symaira-browse/internal/daemon"
	"github.com/danieljustus/symaira-browse/internal/mcp"
)

const oracleCommit = "635dd4050025e410926e940830f1aa7d33229005"

var sourceFiles = []string{
	"go.mod",
	"go.sum",
	"cmd/symbrowse/mcp.go",
	"internal/daemon/client.go",
	"internal/daemon/protocol.go",
	"internal/daemon/redaction.go",
	"internal/daemon/server.go",
	"internal/mcp/profiles.go",
	"internal/mcp/server.go",
	"internal/mcp/tools.go",
}

type manifest struct {
	SchemaVersion int               `json:"schema_version"`
	OracleCommit  string            `json:"oracle_commit"`
	OracleRelease string            `json:"oracle_release"`
	GeneratedBy   string            `json:"generated_by"`
	GeneratorSHA  string            `json:"generator_sha256"`
	SourceFiles   map[string]string `json:"source_files"`
	Fixtures      map[string]entry  `json:"fixtures"`
}

type entry struct {
	Input       string `json:"input"`
	InputSHA256 string `json:"input_sha256"`
	Output      string `json:"output"`
	OutputSHA   string `json:"output_sha256"`
	Profile     string `json:"profile"`
}

var fixtureProfiles = map[string]string{
	"eof":               "core",
	"framed_initialize": "core",
	"initialize":        "core",
	"initialized":       "core",
	"malformed":         "core",
	"missing_argument":  "core",
	"notifications":     "core",
	"tool_error":        "core",
	"tools_all":         "all",
	"tools_core":        "core",
	"tools_nav":         "nav",
	"unknown_method":    "core",
	"unknown_tool":      "core",
}

func main() {
	root := flag.String("root", ".", "Go module and repository root")
	check := flag.Bool("check", false, "verify regenerated outputs and provenance manifest")
	flag.Parse()
	if err := run(*root, *check); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func run(root string, check bool) error {
	root, err := filepath.Abs(root)
	if err != nil {
		return err
	}
	fixtureDir := filepath.Join(root, "testdata", "port", "mcp")
	sourceHashes, err := sourceManifest(root)
	if err != nil {
		return err
	}
	generatorPath := filepath.Join(root, "scripts", "rust-port", "cmd", "mcpfixturegen", "main.go")
	generator, err := os.ReadFile(generatorPath)
	if err != nil {
		return fmt.Errorf("read generator source: %w", err)
	}
	manifestData := manifest{
		SchemaVersion: 1,
		OracleCommit:  oracleCommit,
		OracleRelease: "v0.8.0",
		GeneratedBy:   "scripts/rust-port/cmd/mcpfixturegen",
		GeneratorSHA:  digest(generator),
		SourceFiles:   sourceHashes,
		Fixtures:      make(map[string]entry, len(fixtureProfiles)),
	}
	names := make([]string, 0, len(fixtureProfiles))
	for name := range fixtureProfiles {
		names = append(names, name)
	}
	sort.Strings(names)
	for _, name := range names {
		inputName, outputName := name+".in", name+".out"
		input, err := os.ReadFile(filepath.Join(fixtureDir, inputName))
		if err != nil {
			return fmt.Errorf("read %s: %w", inputName, err)
		}
		output, err := generateOutput(name, fixtureProfiles[name], input)
		if err != nil {
			return fmt.Errorf("generate %s: %w", name, err)
		}
		manifestData.Fixtures[name] = entry{
			Input: inputName, InputSHA256: digest(input),
			Output: outputName, OutputSHA: digest(output), Profile: fixtureProfiles[name],
		}
		outputPath := filepath.Join(fixtureDir, outputName)
		if check {
			current, err := os.ReadFile(outputPath)
			if err != nil {
				return fmt.Errorf("read %s: %w", outputName, err)
			}
			if !bytes.Equal(current, output) {
				return fmt.Errorf("%s differs from Go oracle output; regenerate intentionally", outputName)
			}
		} else if err := os.WriteFile(outputPath, output, 0o644); err != nil {
			return fmt.Errorf("write %s: %w", outputName, err)
		}
	}
	data, err := json.MarshalIndent(manifestData, "", "  ")
	if err != nil {
		return err
	}
	data = append(data, '\n')
	manifestPath := filepath.Join(fixtureDir, "manifest.json")
	if check {
		current, err := os.ReadFile(manifestPath)
		if err != nil {
			return fmt.Errorf("read manifest: %w", err)
		}
		if !bytes.Equal(current, data) {
			return errors.New("MCP fixture provenance manifest is stale; regenerate intentionally")
		}
		fmt.Printf("checked %d Go MCP raw-frame fixtures (%s)\n", len(names), oracleCommit)
		return nil
	}
	if err := os.WriteFile(manifestPath, data, 0o644); err != nil {
		return fmt.Errorf("write manifest: %w", err)
	}
	fmt.Printf("wrote %d Go MCP raw-frame fixtures (%s)\n", len(names), oracleCommit)
	return nil
}

func sourceManifest(root string) (map[string]string, error) {
	files := make(map[string]string, len(sourceFiles))
	for _, relative := range sourceFiles {
		current, err := os.ReadFile(filepath.Join(root, relative))
		if err != nil {
			return nil, fmt.Errorf("read Go oracle source %s: %w", relative, err)
		}
		command := exec.Command("git", "show", oracleCommit+":browse/"+relative)
		command.Dir = root
		pinned, err := command.Output()
		if err != nil {
			return nil, fmt.Errorf("read pinned Go source %s: %w", relative, err)
		}
		if !bytes.Equal(current, pinned) {
			return nil, fmt.Errorf("current Go source differs from pinned oracle: %s", relative)
		}
		files[relative] = digest(current)
	}
	return files, nil
}

func generateOutput(name, profile string, input []byte) ([]byte, error) {
	if runtime.GOOS == "windows" {
		return nil, errors.New("the Go oracle fixture daemon uses Unix sockets and cannot run on Windows")
	}
	temporary, err := os.MkdirTemp("", "symbrowse-mcp-oracle-")
	if err != nil {
		return nil, err
	}
	defer func() { _ = os.RemoveAll(temporary) }()
	socketPath := filepath.Join(temporary, "daemon.sock")
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	server := daemon.NewServer(daemon.Options{
		SocketPath: socketPath,
		Session:    "default",
		PeerValidator: func(net.Conn) error {
			return nil
		},
		Handler: func(_ context.Context, frame daemon.Frame) (any, []daemon.Warning, error) {
			if name == "tool_error" && frame.Cmd == "fetch.url" {
				no := false
				return nil, nil, &daemon.Error{
					Code:                     "fixture_denied",
					Message:                  "fixture daemon denied the request",
					Hint:                     "use an allowed fixture target",
					Details:                  map[string]any{"fixture": true},
					Retryable:                &no,
					RequiresUserConfirmation: &no,
					ResumeHint:               "choose an allowed target",
				}
			}
			return map[string]any{"fixture": name, "command": frame.Cmd}, nil, nil
		},
	})
	serveErr := make(chan error, 1)
	go func() { serveErr <- server.ListenAndServe(ctx) }()
	defer func() {
		cancel()
		_ = server.Close()
		select {
		case <-serveErr:
		case <-time.After(2 * time.Second):
		}
	}()
	if err := waitForSocket(ctx, socketPath, serveErr); err != nil {
		return nil, err
	}
	service, err := mcp.New(mcp.Options{
		Version:    "v0.8.0",
		Session:    "default",
		Executable: "mcp-fixture-generator",
		Profiles:   profile,
		Engine:     "static",
		SocketPath: func(string) (string, error) { return socketPath, nil },
	})
	if err != nil {
		return nil, err
	}
	var output bytes.Buffer
	if err := service.Core().ServeIO(ctx, bytes.NewReader(input), &output); err != nil {
		return nil, fmt.Errorf("serve Go MCP bytes: %w", err)
	}
	return output.Bytes(), nil
}

func waitForSocket(ctx context.Context, path string, done <-chan error) error {
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		connection, err := net.DialTimeout("unix", path, 50*time.Millisecond)
		if err == nil {
			_ = connection.Close()
			return nil
		}
		select {
		case serveErr := <-done:
			return fmt.Errorf("fixture daemon exited before ready: %w", serveErr)
		default:
		}
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-time.After(10 * time.Millisecond):
		}
	}
	return fmt.Errorf("timed out waiting for fixture daemon socket %s", path)
}

func digest(data []byte) string {
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}
