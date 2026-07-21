package main

import (
	"context"
	"flag"
	"fmt"
	"io"
	"os"
	"os/signal"
	"syscall"

	"github.com/danieljustus/symaira-brain/internal/broker"
	"github.com/danieljustus/symaira-brain/internal/config"
	"github.com/danieljustus/symaira-brain/internal/gateway"
	"github.com/danieljustus/symaira-brain/internal/profile"
	"github.com/danieljustus/symaira-corekit/exitcodes"
	"github.com/danieljustus/symaira-corekit/logkit"
)

func cmdServe(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	fs := flag.NewFlagSet("serve", flag.ContinueOnError)
	profileName := fs.String("profile", "", "profile name to serve (required)")
	vaultAgent := fs.String("vault-agent", "", "vault agent name for --stdio mode (default: harness-detected or 'claude-code')")
	fs.SetOutput(stderr)
	if err := fs.Parse(args); err != nil {
		return exitcodes.ExitNoInput
	}

	if *profileName == "" {
		fmt.Fprintln(stderr, "symbrain serve: --profile is required")
		return exitcodes.ExitNoInput
	}

	p, err := profile.Load(*profileName)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain serve: %v\n", err)
		return exitcodes.ExitNoInput
	}

	cfg, err := config.Load()
	if err != nil {
		fmt.Fprintf(stderr, "symbrain serve: load config: %v\n", err)
		return exitcodes.ExitNoInput
	}

	servers := buildServers(p, cfg, stderr, *vaultAgent)

	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer cancel()

	gw := gateway.New(p, servers, logkit.Default())

	if err := gw.ServeIO(ctx, os.Stdin, os.Stdout); err != nil {
		fmt.Fprintf(stderr, "symbrain serve: %v\n", err)
		return exitcodes.ExitGeneric
	}

	for _, ms := range servers {
		ms.Shutdown()
	}

	return exitcodes.ExitOK
}

func buildServers(p *profile.Profile, cfg *config.Config, stderr io.Writer, vaultAgent string) map[string]*broker.ManagedServer {
	servers := make(map[string]*broker.ManagedServer)

	type serverDef struct {
		alias      string
		binaryName string
		override   string
		args       []string
	}

	vaultArgs := []string{"serve", "--allow-locked"}
	if vaultAgent != "" {
		vaultArgs = []string{"serve", "--stdio", "--agent", vaultAgent, "--allow-locked"}
	}

	defs := []serverDef{
		{"vault", "symvault", cfg.Servers.Vault.BinaryPath, vaultArgs},
		{"memory", "symmemory", cfg.Servers.Memory.BinaryPath, []string{"serve"}},
		{"skills", "symskills", cfg.Servers.Skills.BinaryPath, []string{"serve", "--stdio"}},
	}

	for _, d := range defs {
		serverCfg := p.Server(d.alias)
		if !serverCfg.Enabled {
			continue
		}

		path, err := broker.Discover(d.binaryName, d.override)
		if err != nil {
			fmt.Fprintf(stderr, "symbrain serve: %s: %v\n", d.alias, err)
			continue
		}

		ms := broker.NewManagedServer(broker.ServerConfig{
			Name:        d.alias,
			BinaryPath:  path,
			Args:        d.args,
			MaxRestarts: 3,
			Logger:      logkit.Default(),
		})
		servers[d.alias] = ms
	}

	return servers
}
