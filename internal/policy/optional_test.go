package policy

import (
	"testing"

	"github.com/danieljustus/symaira-brain/internal/profile"
)

func TestOptionalModules_DefaultDeniedAndExplicitAllowIsBounded(t *testing.T) {
	cases := []struct {
		alias string
		live  []string
	}{
		{profile.ServerOperate, []string{"version", "permissions_status", "get_policy", "click"}},
		{profile.ServerScope, []string{"scan", "ports_list", "ports_suggest", "mcp_list", "conflicts", "mcp_health", "daemons_list", "write_host"}},
	}
	for _, tc := range cases {
		t.Run(tc.alias, func(t *testing.T) {
			got, err := Evaluate(tc.alias, profile.ServerConfig{Enabled: true}, tc.live)
			if err != nil {
				t.Fatal(err)
			}
			if len(got.Exposed) != 0 {
				t.Fatalf("denied profile exposed %v", got.Exposed)
			}
			got, err = Evaluate(tc.alias, profile.ServerConfig{Enabled: true, ToolsAllow: KnownTools(tc.alias), ToolsDeny: []string{"ports_suggest"}}, tc.live)
			if err != nil {
				t.Fatal(err)
			}
			for _, name := range got.Exposed {
				if name == "click" || name == "write_host" || name == "ports_suggest" {
					t.Fatalf("unsupported/denied tool exposed: %s", name)
				}
			}
		})
	}
}
