package main

import (
	"sort"

	"github.com/danieljustus/symaira-brain/internal/profile"
)

// foreignAccessRisk reports one profile's foreign server (an alias beyond
// the four reserved cores) whose config is shaped to expose nothing:
// access="read" with no tools_read override. Without an explicit
// override, a tool's read/write class falls back to the upstream
// server's readOnlyHint annotation, and then to default_write — most
// upstream servers don't set the annotation, so this shape typically
// yields an empty tool list at runtime (the exact condition
// internal/gateway/server.go's buildCatalog reports as a degradation
// once the profile is actually served).
//
// This is a static, pre-flight signal read from the profile file alone.
// doctor deliberately never spawns a foreign server's configured command
// to find out live — that would mean running an arbitrary user-supplied
// binary on every health check, a materially different trust boundary
// than the four cores this command already knows how to spawn safely.
type foreignAccessRisk struct {
	Profile string `json:"profile"`
	Server  string `json:"server"`
	Detail  string `json:"detail"`
}

func checkForeignAccessRisks() []foreignAccessRisk {
	names, err := profile.ListNames()
	if err != nil {
		return nil
	}

	var out []foreignAccessRisk
	for _, name := range names {
		p, err := profile.Load(name)
		if err != nil {
			continue
		}
		for alias, sc := range p.Servers {
			if profile.IsCoreAlias(alias) || !sc.Enabled {
				continue
			}
			if sc.Access == profile.ForeignAccessRead && len(sc.ToolsRead) == 0 {
				out = append(out, foreignAccessRisk{
					Profile: name,
					Server:  alias,
					Detail:  "access=read with no tools_read override — exposes nothing unless the upstream server sets readOnlyHint on its tools",
				})
			}
		}
	}

	sort.Slice(out, func(i, j int) bool {
		if out[i].Profile != out[j].Profile {
			return out[i].Profile < out[j].Profile
		}
		return out[i].Server < out[j].Server
	})
	return out
}
