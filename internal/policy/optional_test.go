package policy

import (
	"testing"

	"github.com/danieljustus/symaira-brain/internal/profile"
)

func TestOptionalModules_HardMaximumAndExplicitAllowNarrowing(t *testing.T) {
	tests := []struct {
		name, alias string
		live        []string
		allow       []string
		want        []string
		wantUnknown []string
	}{
		{name: "operate click only", alias: profile.ServerOperate, live: []string{"click"}, allow: []string{"click"}, want: []string{}, wantUnknown: []string{"click"}},
		{name: "operate version and click", alias: profile.ServerOperate, live: []string{"version", "click"}, allow: []string{"version", "click"}, want: []string{"version"}, wantUnknown: []string{"click"}},
		{name: "scope write host only", alias: profile.ServerScope, live: []string{"write_host"}, allow: []string{"write_host"}, want: []string{}, wantUnknown: []string{"write_host"}},
		{name: "live unknown without explicit allow", alias: profile.ServerOperate, live: []string{"unknown_upstream_tool"}, want: []string{}, wantUnknown: []string{"unknown_upstream_tool"}},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			cfg := profile.ServerConfig{Enabled: true, ToolsAllow: tc.allow}
			got, err := Evaluate(tc.alias, cfg, tc.live)
			if err != nil {
				t.Fatal(err)
			}
			if !equalStrings(got.Exposed, tc.want) {
				t.Fatalf("Exposed = %v, want %v", got.Exposed, tc.want)
			}
			if !equalStrings(got.Unknown, tc.wantUnknown) {
				t.Fatalf("Unknown = %v, want %v", got.Unknown, tc.wantUnknown)
			}
		})
	}
}

func equalStrings(got, want []string) bool {
	if len(got) != len(want) {
		return false
	}
	for i := range got {
		if got[i] != want[i] {
			return false
		}
	}
	return true
}
