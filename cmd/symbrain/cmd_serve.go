package main

import (
	"context"
	"flag"
	"fmt"
	"io"
	"os"
	"os/signal"
	"path/filepath"
	"strings"
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

func cmdMcp(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	fs := flag.NewFlagSet("mcp", flag.ContinueOnError)
	profileName := fs.String("profile", "", "profile name to serve (required unless --profile-file is given)")
	profileFile := fs.String("profile-file", "", "load the profile from this TOML file instead of the profiles directory")
	vaultAgent := fs.String("vault-agent", "", "vault agent name for --stdio mode (default: harness-detected or 'claude-code')")
	fs.SetOutput(stderr)
	if err := fs.Parse(normalizeFlags(args)); err != nil {
		return exitcodes.ExitNoInput
	}

	p, err := resolveServeProfile(*profileName, *profileFile)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain mcp: %v\n", err)
		return exitcodes.ExitNoInput
	}

	cfg, err := config.Load()
	if err != nil {
		fmt.Fprintf(stderr, "symbrain mcp: load config: %v\n", err)
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
		fmt.Fprintf(stderr, "symbrain mcp: %v\n", err)
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

	vaultArgs := []string{"serve", "--allow-locked"}
	if vaultAgent != "" {
		vaultArgs = []string{"serve", "--stdio", "--agent", vaultAgent, "--allow-locked"}
	}

	defs := []serverDef{
		{"vault", "symvault", cfg.Servers.Vault.BinaryPath, vaultArgs},
	}
	if cfg.Modules.Operate && p.Server(profile.ServerOperate).Enabled {
		if def, err := optionalServerDef(profile.ServerOperate, "symoperate", "operate", cfg.Servers.Operate.BinaryPath); err == nil {
			defs = append(defs, def)
		} else {
			fmt.Fprintf(stderr, "symbrain mcp: %s: %v\n", profile.ServerOperate, err)
		}
	}
	if cfg.Modules.Scope && p.Server(profile.ServerScope).Enabled {
		if def, err := optionalServerDef(profile.ServerScope, "symscope", "scope", cfg.Servers.Scope.BinaryPath); err == nil {
			defs = append(defs, def)
		} else {
			fmt.Fprintf(stderr, "symbrain mcp: %s: %v\n", profile.ServerScope, err)
		}
	}

	for _, d := range defs {
		serverCfg := p.Server(d.alias)
		if !serverCfg.Enabled {
			continue
		}

		path, err := broker.Discover(d.binaryName, d.override)
		if err != nil {
			fmt.Fprintf(stderr, "symbrain mcp: %s: %v\n", d.alias, err)
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

	// Foreign servers: a profile may name any stdio MCP server beyond the
	// four cores (ADR 0001, D2/D4). URL-only foreign servers have no stdio
	// transport yet — the HTTP MCP client is not built — so they are warned
	// and skipped rather than failing the whole serve.
	for alias, serverCfg := range p.Servers {
		if profile.IsCoreAlias(alias) || !serverCfg.Enabled {
			continue
		}
		if serverCfg.Command == "" {
			fmt.Fprintf(stderr, "symbrain mcp: %s: url-only foreign server (no stdio transport yet); skipping\n", alias)
			continue
		}

		var path string
		var err error
		if strings.ContainsRune(serverCfg.Command, os.PathSeparator) {
			path, err = broker.Discover(filepath.Base(serverCfg.Command), serverCfg.Command)
		} else {
			path, err = broker.Discover(serverCfg.Command, "")
		}
		if err != nil {
			fmt.Fprintf(stderr, "symbrain mcp: %s: %v\n", alias, err)
			continue
		}

		ms := broker.NewManagedServer(broker.ServerConfig{
			Name:        alias,
			BinaryPath:  path,
			Args:        serverCfg.Args,
			MaxRestarts: 3,
			Logger:      logkit.Default(),
		})
		servers[alias] = ms
	}

	return servers
}

// optionalServerDef prefers the consolidated direct binary when no explicit
// override is configured. If the direct binary is unavailable, it falls back
// to the legacy symcockpit subcommand. An explicit override is authoritative:
// an invalid override degrades the module and never falls back.
type serverDef struct {
	alias      string
	binaryName string
	override   string
	args       []string
}

func optionalServerDef(alias, directBinary, cockpitCommand, override string) (serverDef, error) {
	if override != "" {
		if _, err := broker.Discover(directBinary, override); err != nil {
			return serverDef{}, fmt.Errorf("invalid explicit binary override: %w", err)
		}
		return serverDef{alias, directBinary, override, []string{"serve"}}, nil
	}
	if path, err := broker.Discover(directBinary, ""); err == nil {
		return serverDef{alias, directBinary, path, []string{"serve"}}, nil
	}
	if path, err := broker.Discover("symcockpit", ""); err == nil {
		return serverDef{alias, "symcockpit", path, []string{cockpitCommand, "serve"}}, nil
	}
	return serverDef{}, fmt.Errorf("neither %s nor symcockpit was found", directBinary)
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
		fmt.Fprintf(stderr, "symbrain mcp: open memory db: %v\n", err)
		return nil
	}

	memjwt, err := memorysecurity.NewJWTProvider(memcfg, memdb)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain mcp: init memory JWT provider: %v\n", err)
		return nil
	}

	memsrv := memorymcp.NewServer(memdb, memjwt, version, memcfg)
	memsrv.SetClientIDOverride(p.Name)
	return memsrv
}
