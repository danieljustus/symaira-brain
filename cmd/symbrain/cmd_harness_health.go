package main

import (
	"context"
	"flag"
	"fmt"
	"io"
	"sort"
	"sync"
	"time"

	"github.com/danieljustus/symaira-brain/internal/broker"
	"github.com/danieljustus/symaira-brain/internal/harness"
	"github.com/danieljustus/symaira-brain/internal/output"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// healthTimeout bounds each server's MCP initialize handshake probe. It is
// deliberately shorter than doctor's handshake timeout: harness health
// probes many servers and must not stall on a wedged one.
const healthTimeout = 5 * time.Second

// harnessHealthEntry is one server's handshake probe result.
type harnessHealthEntry struct {
	Harness   string `json:"harness"`
	Config    string `json:"config"`
	Server    string `json:"server"`
	Transport string `json:"transport"`
	Healthy   bool   `json:"healthy"`
	Error     string `json:"error,omitempty"`
}

// harnessHealthReport is the full result of `symbrain harness health`.
type harnessHealthReport struct {
	Servers []harnessHealthEntry `json:"servers"`
}

func cmdHarnessHealth(args []string, stdout, stderr io.Writer, format output.Format) exitcodes.ExitCode {
	fs := flag.NewFlagSet("harness health", flag.ContinueOnError)
	harnessName := fs.String("harness", "", "only probe servers of this harness")
	projectDir := fs.String("project", "", "project directory to inspect for project-local harness config")
	fs.SetOutput(stderr)
	if err := fs.Parse(normalizeFlags(args)); err != nil {
		return exitcodes.ExitNoInput
	}
	if fs.NArg() > 0 {
		fmt.Fprintf(stderr, "symbrain harness health: unexpected argument %q\n", fs.Arg(0))
		return exitcodes.ExitNoInput
	}

	report := probeHarnessHealth(*harnessName, *projectDir)

	rows := output.Rows{
		JSON: report,
		Table: func(w io.Writer) error {
			printHarnessHealth(w, report)
			return nil
		},
	}
	if err := output.Render(stdout, format, rows); err != nil {
		fmt.Fprintf(stderr, "symbrain harness health: format output: %v\n", err)
		return exitcodes.ExitGeneric
	}
	return exitcodes.ExitOK
}

// probeHarnessHealth probes the MCP handshake of every server registered in
// every harness config (or a single harness when name is non-empty). Probes
// run concurrently and are individually bounded; a wedged server never
// blocks the others.
//
// "Healthy" means the server answered an MCP initialize handshake — a
// correct stdio server blocks on stdin and must not be reported unhealthy.
// url/transport-only servers cannot be probed over the stdio transport and
// are reported as skipped with the transport in the error.
func probeHarnessHealth(name, projectDir string) harnessHealthReport {
	inventory := harness.List(projectDir)

	var entries []harnessHealthEntry
	var mu sync.Mutex
	var wg sync.WaitGroup

	collect := func(hName string, cfg harness.ConfigInventory) {
		for _, info := range cfg.Servers {
			if info.Transport != "stdio" || info.Command == "" {
				// The skip path appends from the caller's goroutine while
				// probe goroutines write entries concurrently, so it must
				// share the same mutex (race detector, -race CI gate).
				mu.Lock()
				entries = append(entries, harnessHealthEntry{
					Harness:   hName,
					Config:    cfg.Path,
					Server:    info.Name,
					Transport: info.Transport,
					Healthy:   false,
					Error:     "not probed: " + info.Transport + " transport is not stdio",
				})
				mu.Unlock()
				continue
			}
			wg.Add(1)
			go func(hName string, info harness.ServerInfo) {
				defer wg.Done()
				mu.Lock()
				entries = append(entries, probeServerHandshake(hName, cfg, info))
				mu.Unlock()
			}(hName, info)
		}
	}

	for _, item := range inventory.Harnesses {
		if name != "" && string(item.Name) != name {
			continue
		}
		collect(string(item.Name), item.Global)
		if item.Project != nil {
			collect(string(item.Name), *item.Project)
		}
	}
	wg.Wait()

	sort.Slice(entries, func(i, j int) bool {
		if entries[i].Harness != entries[j].Harness {
			return entries[i].Harness < entries[j].Harness
		}
		if entries[i].Server != entries[j].Server {
			return entries[i].Server < entries[j].Server
		}
		return entries[i].Config < entries[j].Config
	})
	return harnessHealthReport{Servers: entries}
}

func probeServerHandshake(hName string, cfg harness.ConfigInventory, info harness.ServerInfo) harnessHealthEntry {
	entry := harnessHealthEntry{
		Harness:   hName,
		Config:    cfg.Path,
		Server:    info.Name,
		Transport: info.Transport,
	}

	path, err := broker.Discover(info.Command, "")
	if err != nil {
		entry.Error = "discover: " + err.Error()
		return entry
	}

	ctx, cancel := context.WithTimeout(context.Background(), healthTimeout)
	defer cancel()

	c, err := broker.Spawn(path, broker.Options{Args: info.Args, Stderr: io.Discard})
	if err != nil {
		entry.Error = "spawn: " + err.Error()
		return entry
	}
	defer func() {
		_ = c.Close()
		if p := c.Process(); p != nil {
			_ = p.Kill()
		}
	}()

	if _, err := c.Initialize(ctx); err != nil {
		entry.Error = "initialize: " + err.Error()
		return entry
	}
	entry.Healthy = true
	return entry
}

func printHarnessHealth(w io.Writer, report harnessHealthReport) {
	if len(report.Servers) == 0 {
		fmt.Fprintln(w, "no MCP servers found")
		return
	}
	for _, s := range report.Servers {
		if s.Healthy {
			fmt.Fprintf(w, "  ✓  %-12s %-14s %s (stdio)\n", s.Harness, s.Server, s.Config)
		} else {
			fmt.Fprintf(w, "  ✗  %-12s %-14s %s: %s\n", s.Harness, s.Server, s.Config, s.Error)
		}
	}
}
