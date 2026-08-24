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
	memoryconfig "github.com/danieljustus/symaira-brain/internal/memory/config"
	memorydb "github.com/danieljustus/symaira-brain/internal/memory/db"
	memorymcp "github.com/danieljustus/symaira-brain/internal/memory/mcp"
	memorysecurity "github.com/danieljustus/symaira-brain/internal/memory/security"
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

	// The memory core is embedded in-process (repo consolidation step 4
	// phase 2b): open its SQLite DB + JWT provider and build its MCP server
	// directly instead of spawning a memory child.
	memoryServer := buildMemoryServer(p, stderr, version)

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
	gw.SetMemoryServer(memoryServer)

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

// buildMemoryServer opens the embedded memory runtime (config + SQLite DB
// + JWT provider) and returns its MCP server, attributed to the given brain
// profile. It returns nil when the memory core cannot be initialized, so the
// gateway degrades gracefully (memory tools simply absent) instead of failing
// the whole serve.
func buildMemoryServer(p *profile.Profile, stderr io.Writer, version string) *memorymcp.Server {
	memcfg, err := memoryconfig.Load()
	if err != nil {
		memcfg = memoryconfig.Defaults()
	}

	memdb, err := memorydb.Open(memcfg)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain serve: open memory db: %v\n", err)
		return nil
	}

	memjwt, err := memorysecurity.NewJWTProvider(memcfg, memdb)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain serve: init memory JWT provider: %v\n", err)
		return nil
	}

	memsrv := memorymcp.NewServer(memdb, memjwt, version, memcfg)
	memsrv.SetClientIDOverride(p.Name)
	return memsrv
}
