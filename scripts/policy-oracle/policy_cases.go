package main

import "github.com/danieljustus/symaira-brain/internal/policy"

func getPolicyTestCases() []PolicyTestCase {
	vaultFull := policy.KnownTools("vault")
	vaultLiveWithUnknown := append(append([]string(nil), vaultFull...), "vault_extra_tool")

	memoryFull := policy.KnownTools("memory")
	memoryLiveWithActivity := append(append([]string(nil), memoryFull...), "activity_get", "activity_search", "activity_status", "mem_extra")

	return []PolicyTestCase{
		{
			ID:        "pol_vault_request_only",
			Server:    "vault",
			Config:    ExpectedServer{Enabled: true, Mode: "request_only"},
			LiveTools: vaultLiveWithUnknown,
		},
		{
			ID:        "pol_vault_full",
			Server:    "vault",
			Config:    ExpectedServer{Enabled: true, Mode: "full"},
			LiveTools: vaultLiveWithUnknown,
		},
		{
			ID:        "pol_vault_off",
			Server:    "vault",
			Config:    ExpectedServer{Enabled: true, Mode: "off"},
			LiveTools: vaultLiveWithUnknown,
		},
		{
			ID:        "pol_vault_disabled",
			Server:    "vault",
			Config:    ExpectedServer{Enabled: false, Mode: "full"},
			LiveTools: vaultLiveWithUnknown,
		},
		{
			ID:     "pol_vault_allow_overrides",
			Server: "vault",
			Config: ExpectedServer{
				Enabled:    true,
				Mode:       "request_only",
				ToolsAllow: []string{"health"},
			},
			LiveTools: vaultLiveWithUnknown,
		},
		{
			ID:     "pol_vault_deny_removes",
			Server: "vault",
			Config: ExpectedServer{
				Enabled:   true,
				Mode:      "request_only",
				ToolsDeny: []string{"health"},
			},
			LiveTools: vaultLiveWithUnknown,
		},
		{
			ID:     "pol_vault_deny_wins_over_allow",
			Server: "vault",
			Config: ExpectedServer{
				Enabled:    true,
				Mode:       "request_only",
				ToolsAllow: []string{"health", "generate_password"},
				ToolsDeny:  []string{"health"},
			},
			LiveTools: vaultLiveWithUnknown,
		},
		{
			ID:        "pol_memory_read_only",
			Server:    "memory",
			Config:    ExpectedServer{Enabled: true, Mode: "read_only"},
			LiveTools: memoryLiveWithActivity,
		},
		{
			ID:        "pol_memory_read_write",
			Server:    "memory",
			Config:    ExpectedServer{Enabled: true, Mode: "read_write"},
			LiveTools: memoryLiveWithActivity,
		},
		{
			ID:     "pol_memory_activity_explicit_allow",
			Server: "memory",
			Config: ExpectedServer{
				Enabled:    true,
				Mode:       "read_only",
				ToolsAllow: []string{"memory_get", "activity_get", "activity_status"},
			},
			LiveTools: memoryLiveWithActivity,
		},
		{
			ID:        "pol_skills_full_live",
			Server:    "skills",
			Config:    ExpectedServer{Enabled: true},
			LiveTools: []string{"skill_search", "skill_install", "custom_skill"},
		},
		{
			ID:     "pol_skills_allow_narrowing",
			Server: "skills",
			Config: ExpectedServer{
				Enabled:    true,
				ToolsAllow: []string{"skill_search"},
			},
			LiveTools: []string{"skill_search", "skill_install", "custom_skill"},
		},
		{
			ID:     "pol_skills_deny",
			Server: "skills",
			Config: ExpectedServer{
				Enabled:   true,
				ToolsDeny: []string{"custom_skill"},
			},
			LiveTools: []string{"skill_search", "skill_install", "custom_skill"},
		},
		{
			ID:        "pol_usage_single_tool",
			Server:    "usage",
			Config:    ExpectedServer{Enabled: true},
			LiveTools: []string{"get_ai_usage", "unknown_usage_tool"},
		},
		{
			ID:     "pol_usage_deny",
			Server: "usage",
			Config: ExpectedServer{
				Enabled:   true,
				ToolsDeny: []string{"get_ai_usage"},
			},
			LiveTools: []string{"get_ai_usage", "unknown_usage_tool"},
		},
		{
			ID:        "pol_operate_allow_cannot_widen",
			Server:    "operate",
			Config:    ExpectedServer{Enabled: true, ToolsAllow: []string{"version", "click"}},
			LiveTools: []string{"version", "click"},
		},
		{
			ID:        "pol_scope_write_host_rejected",
			Server:    "scope",
			Config:    ExpectedServer{Enabled: true, ToolsAllow: []string{"write_host"}},
			LiveTools: []string{"write_host"},
		},
		{
			ID:        "pol_operate_unknown_without_allow",
			Server:    "operate",
			Config:    ExpectedServer{Enabled: true},
			LiveTools: []string{"unknown_upstream_tool"},
		},
		{
			ID:         "pol_preset_eval_vault",
			Server:     "vault",
			Config:     ExpectedServer{Enabled: true, Mode: "request_only"},
			PresetEval: true,
		},
		{
			ID:         "pol_preset_eval_memory",
			Server:     "memory",
			Config:     ExpectedServer{Enabled: true, Mode: "read_write"},
			PresetEval: true,
		},
		{
			ID:        "pol_foreign_default_access_write",
			Server:    "myforeign",
			Config:    ExpectedServer{Enabled: true, Command: "/usr/bin/tool"},
			IsForeign: true,
			ForeignTools: []ForeignToolCase{
				{Name: "tool_alpha"},
				{Name: "tool_beta"},
			},
		},
		{
			ID:        "pol_foreign_access_read_unannotated",
			Server:    "myforeign",
			Config:    ExpectedServer{Enabled: true, Access: "read", Command: "/usr/bin/tool"},
			IsForeign: true,
			ForeignTools: []ForeignToolCase{
				{Name: "tool_alpha"},
				{Name: "tool_beta"},
			},
		},
		{
			ID:        "pol_foreign_access_read_annotated_hint_read",
			Server:    "myforeign",
			Config:    ExpectedServer{Enabled: true, Access: "read", Command: "/usr/bin/tool"},
			IsForeign: true,
			ForeignTools: []ForeignToolCase{
				{Name: "tool_ro", ReadOnlyHint: boolPtr(true)},
				{Name: "tool_rw", ReadOnlyHint: boolPtr(false)},
				{Name: "tool_none"},
			},
		},
		{
			ID:     "pol_foreign_access_read_override_tools_read",
			Server: "myforeign",
			Config: ExpectedServer{
				Enabled:   true,
				Access:    "read",
				Command:   "/usr/bin/tool",
				ToolsRead: []string{"tool_rw"},
			},
			IsForeign: true,
			ForeignTools: []ForeignToolCase{
				{Name: "tool_ro", ReadOnlyHint: boolPtr(true)},
				{Name: "tool_rw", ReadOnlyHint: boolPtr(false)},
			},
		},
		{
			ID:     "pol_foreign_access_write_override_tools_write",
			Server: "myforeign",
			Config: ExpectedServer{
				Enabled:    true,
				Access:     "write",
				Command:    "/usr/bin/tool",
				ToolsWrite: []string{"tool_ro"},
			},
			IsForeign: true,
			ForeignTools: []ForeignToolCase{
				{Name: "tool_ro", ReadOnlyHint: boolPtr(true)},
			},
		},
		{
			ID:     "pol_foreign_deny_wins",
			Server: "myforeign",
			Config: ExpectedServer{
				Enabled:    true,
				Access:     "write",
				Command:    "/usr/bin/tool",
				ToolsAllow: []string{"tool_a", "tool_b"},
				ToolsDeny:  []string{"tool_a"},
			},
			IsForeign: true,
			ForeignTools: []ForeignToolCase{
				{Name: "tool_a"},
				{Name: "tool_b"},
			},
		},
		{
			ID:        "pol_foreign_disabled",
			Server:    "myforeign",
			Config:    ExpectedServer{Enabled: false, Command: "/usr/bin/tool"},
			IsForeign: true,
			ForeignTools: []ForeignToolCase{
				{Name: "tool_a"},
			},
		},
		{
			ID:        "pol_foreign_empty_read",
			Server:    "myforeign",
			Config:    ExpectedServer{Enabled: true, Access: "read", Command: "/usr/bin/tool"},
			IsForeign: true,
		},
		{
			ID:        "pol_foreign_empty_write",
			Server:    "myforeign",
			Config:    ExpectedServer{Enabled: true, Access: "write", Command: "/usr/bin/tool"},
			IsForeign: true,
		},
		{
			ID:        "pol_foreign_empty_disabled",
			Server:    "myforeign",
			Config:    ExpectedServer{Enabled: false, Command: "/usr/bin/tool"},
			IsForeign: true,
		},
	}
}
