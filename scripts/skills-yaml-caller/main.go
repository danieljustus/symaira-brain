// Command skills-yaml-caller exercises real status/dry-run comparison caches.
package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"runtime"

	"github.com/danieljustus/symaira-brain/internal/skills/install"
	"github.com/danieljustus/symaira-brain/internal/skills/render"
)

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func run() error {
	root := flag.String("root", "", "owned, preseeded isolated root")
	cache := flag.String("cache", "default", "default, empty or explicit")
	flag.Parse()
	if !filepath.IsAbs(*root) {
		return fmt.Errorf("absolute isolated root required")
	}
	opts := install.StatusOptions{HomeDir: filepath.Join(*root, "home"), LibraryDir: filepath.Join(*root, "library"), BaseDir: filepath.Join(*root, "base"), Targets: []render.Target{render.Target("opencode")}}
	switch *cache {
	case "explicit":
		opts.CacheDir = filepath.Join(*root, "comparison-cache")
	case "empty":
		opts.CacheDir = ""
	case "default":
	default:
		return fmt.Errorf("unknown cache setting")
	}
	rows, err := install.Status(opts)
	if err != nil {
		return err
	}
	if len(rows) != 1 || rows[0].Status != install.StatusStale || rows[0].Error != "" {
		return fmt.Errorf("unexpected status: %+v", rows)
	}
	plans := install.Sync(rows, install.SyncOptions{HomeDir: opts.HomeDir, LibraryDir: opts.LibraryDir, BaseDir: opts.BaseDir, RenderDir: filepath.Join(*root, "retained-render"), DryRun: true})
	if len(plans) != 1 || plans[0].Action != "planned" || plans[0].Error != "" {
		return fmt.Errorf("unexpected plan: %+v", plans)
	}
	return json.NewEncoder(os.Stdout).Encode(map[string]any{"success": true, "platform": runtime.GOOS, "arch": runtime.GOARCH, "status": rows, "dry_run": plans})
}
