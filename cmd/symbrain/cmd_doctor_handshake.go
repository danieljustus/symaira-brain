package main

import (
	"context"
	"fmt"
	"io"
	"time"

	"github.com/danieljustus/symaira-brain/internal/broker"
	"github.com/danieljustus/symaira-brain/internal/config"
	"github.com/danieljustus/symaira-brain/internal/policy"
	"github.com/danieljustus/symaira-brain/internal/profile"
)

// Profile handshake checking for `symbrain doctor`: spawns each enabled
// server of each discovered profile and probes the MCP handshake. Split
// from cmd_doctor.go to keep every file under the 400-line rule.

const handshakeTimeout = 5 * time.Second

func checkHandshakes(ctx context.Context, vaultAgent string) []profileHandshake {
	names, err := profile.ListNames()
	if err != nil || len(names) == 0 {
		return nil
	}

	cfg, cfgErr := config.Load()
	if cfgErr != nil {
		cfg = config.Defaults()
	}

	var results []profileHandshake
	for _, name := range names {
		p, err := profile.Load(name)
		if err != nil {
			continue
		}

		// Memory and skills are embedded cores. Only vault remains an external
		// process whose MCP handshake can be probed here.
		for _, alias := range []string{profile.ServerVault} {
			serverCfg := p.Server(alias)
			if !serverCfg.Enabled {
				continue
			}

			binaryName := aliasBinary(alias)
			override := ""
			override = cfg.Servers.Vault.BinaryPath

			path, err := broker.Discover(binaryName, override)
			if err != nil {
				results = append(results, profileHandshake{
					Profile: name,
					Server:  alias,
					Error:   err.Error(),
				})
				continue
			}

			h := probeHandshake(ctx, path, name, alias, vaultAgent, serverCfg)
			results = append(results, h)
		}
	}
	return results
}

func aliasBinary(alias string) string {
	switch alias {
	case profile.ServerVault:
		return "symvault"
	default:
		return alias
	}
}

func probeHandshake(ctx context.Context, path, profileName, alias, vaultAgent string, serverCfg profile.ServerConfig) profileHandshake {
	h := profileHandshake{Profile: profileName, Server: alias}

	handshakeCtx, cancel := context.WithTimeout(ctx, handshakeTimeout)
	defer cancel()

	args := []string{"serve"}
	if alias == profile.ServerSkills {
		args = []string{"serve", "--stdio"}
	}
	if alias == profile.ServerVault && vaultAgent != "" {
		args = []string{"serve", "--stdio", "--agent", vaultAgent, "--allow-locked"}
	}
	c, err := broker.Spawn(path, broker.Options{Args: args, Stderr: io.Discard})
	if err != nil {
		h.Error = fmt.Sprintf("spawn: %v", err)
		return h
	}
	defer func() {
		_ = c.Close()
		if p := c.Process(); p != nil {
			_ = p.Kill()
		}
	}()

	result, err := c.Initialize(handshakeCtx)
	if err != nil {
		h.Error = fmt.Sprintf("initialize: %v", err)
		return h
	}

	h.Protocol = result.ProtocolVersion

	tools, err := c.ListTools(handshakeCtx)
	if err != nil {
		h.Error = fmt.Sprintf("tools/list: %v", err)
		return h
	}

	h.ToolCount = len(tools)

	liveNames := make([]string, len(tools))
	for i, t := range tools {
		liveNames[i] = t.Name
	}

	report, err := policy.Evaluate(alias, serverCfg, liveNames)
	if err != nil {
		h.Error = fmt.Sprintf("policy: %v", err)
		return h
	}

	h.Exposed = len(report.Exposed)
	h.Hidden = len(report.Hidden)
	h.Unknown = len(report.Unknown)

	return h
}
