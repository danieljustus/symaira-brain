// Probe the unmodified frozen internal/config loader in an owned module copy.
// This fixture is not Go production code or a replacement for full CLI proof.
package main

import (
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"

	"github.com/danieljustus/symaira-brain/internal/config"
	"github.com/danieljustus/symaira-corekit/configkit"
)

func main() {
	c, err := config.Load()
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(2)
	}
	h := func(s string) string { return hex.EncodeToString([]byte(s)) }
	value := map[string]any{
		"config_path_hex":                 h(configkit.DefaultPath(config.AppName)),
		"default_profile_hex":             h(c.DefaultProfile),
		"audit.enabled":                   c.Audit.Enabled,
		"audit.verbose":                   c.Audit.Verbose,
		"gateway.identity_injection":      c.Gateway.IdentityInjection,
		"updatecheck.enabled":             c.UpdateCheck.Enabled,
		"servers.vault.binary_path_hex":   h(c.Servers.Vault.BinaryPath),
		"servers.operate.binary_path_hex": h(c.Servers.Operate.BinaryPath),
		"servers.scope.binary_path_hex":   h(c.Servers.Scope.BinaryPath),
		"patterns.enabled":                c.Patterns.Enabled,
		"patterns.promotion_threshold":    c.Patterns.PromotionThreshold,
		"modules.browse":                  c.Modules.Browse,
		"modules.operate":                 c.Modules.Operate,
		"modules.scope":                   c.Modules.Scope,
	}
	if err := json.NewEncoder(os.Stdout).Encode(value); err != nil {
		fmt.Fprintf(os.Stderr, "encode JSON: %v\n", err)
		os.Exit(1)
	}
}
