package main

import (
	"bytes"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/config"
	"github.com/danieljustus/symaira-brain/internal/profile"
)

func TestBuildServers_OptionalModulesDisabledIndependentlySkipDiscovery(t *testing.T) {
	t.Setenv("PATH", t.TempDir())
	cases := []struct {
		name   string
		config config.ModulesConfig
	}{
		{name: "operate disabled", config: config.ModulesConfig{Scope: true}},
		{name: "scope disabled", config: config.ModulesConfig{Operate: true}},
		{name: "both disabled", config: config.ModulesConfig{}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			p := &profile.Profile{Name: tc.name, Servers: profile.Servers{
				profile.ServerOperate: {Enabled: true},
				profile.ServerScope:   {Enabled: true},
			}}
			var stderr bytes.Buffer
			servers := buildServers(p, &config.Config{Modules: tc.config}, &stderr, "")
			if tc.config.Operate == false && tc.config.Scope == false {
				if len(servers) != 0 || stderr.Len() != 0 {
					t.Fatalf("both modules disabled: servers=%v stderr=%q", servers, stderr.String())
				}
				return
			}
			if len(servers) != 0 {
				t.Fatalf("servers = %v, want missing-binary optional worker skipped", servers)
			}
			wantWarning := profile.ServerOperate
			if tc.config.Operate == false {
				wantWarning = profile.ServerScope
			}
			if !strings.Contains(stderr.String(), wantWarning) {
				t.Fatalf("stderr = %q, want discovery only for enabled %s", stderr.String(), wantWarning)
			}
		})
	}
}
