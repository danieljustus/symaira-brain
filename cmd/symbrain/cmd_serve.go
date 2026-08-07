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
	profileName := fs.String("profile", "", "profile name to serve (required unless --profile-file is given)")
	profileFile := fs.String("profile-file", "", "load the profile from this TOML file instead of the profiles directory")
	vaultAgent := fs.String("vault-agent", "", "vault agent name for --stdio mode (default: harness-detected or 'claude-code')")
	fs.SetOutput(stderr)
	if err := fs.Parse(args); err != nil {
		return exitcodes.ExitNoInput
	}

	p, err := resolveServeProfile(*profileName, *profileFile)
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

	// Defer shutdown of all managed servers so child processes are
	// always cleaned up, even when ServeIO returns an error.
	defer func() {
		for _, ms := range servers {
			ms.Shutdown()
		}
	}()

	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer cancel()

	gw := gateway.New(p, servers, logkit.Default(), cfg, version)

	if err := gw.ServeIO(ctx, os.Stdin, os.Stdout); err != nil {
		fmt.Fprintf(stderr, "symbrain serve: %v\n", err)
		return exitcodes.ExitGeneric
	}

	return exitcodes.ExitOK
}

// resolveServeProfile loads the profile from exactly one of the two
// mutually exclusive sources: the profiles directory (by name) or an
// explicit TOML file (--profile-file, e.g. a room-local profile).
func resolveServeProfile(name, file string) (*profile.Profile, error) {
	if name == "" && file == "" {
		return nil, fmt.Errorf("--profile is required (or --profile-file <path>)")
	}
	if name != "" && file != "" {
		return nil, fmt.Errorf("--profile and --profile-file are mutually exclusive")
	}
	if file != "" {
		return profile.LoadFile(file)
	}
	return profile.Load(name)
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
