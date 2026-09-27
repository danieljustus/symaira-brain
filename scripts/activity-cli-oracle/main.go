// Command activity-cli-oracle pins the production Go activity status command
// in an isolated home and empty SQLite database.
package main

import (
	"bytes"
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"time"
)

type result struct {
	Args     []string `json:"args"`
	ExitCode int      `json:"exit_code"`
	Stdout   string   `json:"stdout"`
	Stderr   string   `json:"stderr"`
}

func main() {
	check := flag.Bool("check", false, "check the checked-in Go oracle fixture")
	goBinary := flag.String("go-binary", "", "production Go binary built from the pinned source")
	output := flag.String("output", "rust/symbrain-cli/tests/fixtures/activity_cli_oracle.json", "fixture path")
	flag.Parse()
	if *goBinary == "" {
		fatal("-go-binary is required")
	}

	data, err := json.MarshalIndent(run(*goBinary), "", "  ")
	if err != nil {
		fatal(err.Error())
	}
	data = append(data, '\n')
	if *check {
		current, err := os.ReadFile(*output)
		if err != nil || !bytes.Equal(data, current) {
			fatal(*output + " is stale; regenerate with scripts/activity-cli-oracle")
		}
		fmt.Println("PASS: activity status Go CLI oracle is current")
		return
	}
	if err := os.MkdirAll(filepath.Dir(*output), 0o755); err != nil {
		fatal(err.Error())
	}
	if err := os.WriteFile(*output, data, 0o644); err != nil {
		fatal(err.Error())
	}
	fmt.Printf("Wrote %s\n", *output)
}

func run(binary string) result {
	root, err := os.MkdirTemp("", "symbrain-activity-cli-oracle.")
	if err != nil {
		fatal(err.Error())
	}
	defer os.RemoveAll(root)
	if resolved, err := filepath.EvalSymlinks(root); err == nil {
		root = resolved
	}
	home := filepath.Join(root, "home")
	config := filepath.Join(home, ".config")
	profileDir := filepath.Join(config, "symbrain", "profiles")
	databaseDir := filepath.Join(root, "data")
	for _, dir := range []string{profileDir, databaseDir} {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			fatal(err.Error())
		}
	}
	profile := "[profile]\nname = \"activity-oracle\"\n\n[servers.memory]\nenabled = true\nmode = \"read_only\"\ntools_allow = [\"activity_search\", \"activity_get\", \"activity_status\"]\n"
	if err := os.WriteFile(filepath.Join(profileDir, "activity-oracle.toml"), []byte(profile), 0o600); err != nil {
		fatal(err.Error())
	}
	dbPath := filepath.Join(databaseDir, "activity.db")
	args := []string{"activity", "status", "--profile=activity-oracle", "--max-tokens=40", "--db=" + dbPath}
	return runCase(binary, root, home, config, args)
}

func runCase(binary, root, home, config string, args []string) result {
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	cmd := exec.CommandContext(ctx, binary, args...)
	cmd.Env = []string{"HOME=" + home, "XDG_CONFIG_HOME=" + config, "XDG_DATA_HOME=" + filepath.Join(root, "data"), "XDG_CACHE_HOME=" + filepath.Join(root, "cache"), "XDG_STATE_HOME=" + filepath.Join(root, "state")}
	if runtime.GOOS == "windows" {
		for _, key := range []string{"SystemRoot", "SYSTEMROOT", "windir", "WINDIR", "ComSpec", "COMSPEC"} {
			if value, ok := os.LookupEnv(key); ok {
				cmd.Env = append(cmd.Env, key+"="+value)
			}
		}
	}
	var stdout, stderr bytes.Buffer
	cmd.Stdout, cmd.Stderr = &stdout, &stderr
	err := cmd.Run()
	if ctx.Err() == context.DeadlineExceeded {
		fatal("activity status command exceeded timeout")
	}
	exitCode := 0
	if err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			exitCode = exitErr.ExitCode()
		} else {
			exitCode = 1
		}
	}
	for i := range args {
		args[i] = strings.ReplaceAll(args[i], root, "<root>")
		args[i] = strings.ReplaceAll(args[i], `\`, "/")
	}
	return result{Args: args, ExitCode: exitCode, Stdout: stdout.String(), Stderr: stderr.String()}
}

func fatal(message string) {
	fmt.Fprintln(os.Stderr, "activity-cli-oracle:", message)
	os.Exit(1)
}
