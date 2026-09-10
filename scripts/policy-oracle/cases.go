package main

type ProfileTestCase struct {
	ID   string `json:"id"`
	Name string `json:"name"`
	TOML string `json:"toml"`
}

func getProfileTestCases() []ProfileTestCase {
	cases := []ProfileTestCase{
		{
			ID:   "prof_personal",
			Name: "personal",
			TOML: `[profile]
name = "personal"
description = "Full access for trusted personal use"

[servers.vault]
enabled = true
mode = "full"

[servers.memory]
enabled = true
mode = "read_write"

[servers.skills]
enabled = true

[servers.usage]
enabled = true

[audit]
enabled = true
`,
		},
		{
			ID:   "prof_restricted",
			Name: "restricted",
			TOML: `[profile]
name = "restricted"
description = "Least-privilege profile"

[servers.vault]
enabled = true
mode = "request_only"

[servers.memory]
enabled = true
mode = "read_only"

[servers.skills]
enabled = true

[servers.usage]
enabled = true

[audit]
enabled = true
`,
		},
		{
			ID:   "prof_foreign_read_only",
			Name: "foreign-read-only",
			TOML: `[profile]
name = "foreign-read-only"
description = "Foreign server read-only access"

[servers.vault]
enabled = true
mode = "request_only"

[servers.memory]
enabled = true
mode = "read_only"

[servers.skills]
enabled = true

[servers.usage]
enabled = true

[servers.docs]
enabled = true
command = "/usr/local/bin/docs-mcp"
access = "read"
tools_read = ["search", "get"]

[audit]
enabled = true
`,
		},
		{
			ID:   "prof_minimal_defaults",
			Name: "minimal",
			TOML: `[profile]
name = "minimal"
`,
		},
		{
			ID:   "prof_server_without_mode_defaults",
			Name: "no-mode",
			TOML: `[profile]
name = "no-mode"

[servers.vault]
enabled = true

[servers.memory]
enabled = true
`,
		},
		{
			ID:   "prof_inline_all",
			Name: "inline-all",
			TOML: `profile = { name = "inline-all", description = "fully inline" }
audit = { enabled = false }
servers = { vault = { enabled = true, mode = "full" }, memory = { enabled = true, mode = "read_write" }, skills = { enabled = true }, usage = { enabled = true } }
`,
		},
		{
			ID:   "prof_nested_inline_and_dotted",
			Name: "combo",
			TOML: `[profile]
name = "combo"

[servers]
vault = { enabled = true, mode = "request_only" }
memory = { enabled = true, mode = "read_only" }

[servers.echo]
command = "/bin/echo"
args = ["test"]
`,
		},
		{
			ID:   "prof_foreign_url",
			Name: "foreign-url",
			TOML: `[profile]
name = "foreign-url"

[servers.fig]
enabled = true
url = "https://mcp.example.com/sse"
`,
		},
		{
			ID:   "prof_foreign_default_access",
			Name: "foreign-def-access",
			TOML: `[profile]
name = "foreign-def-access"

[servers.myapi]
enabled = true
command = "/usr/bin/myapi"
`,
		},
		{
			ID:   "prof_warnings_unknown_keys",
			Name: "warn-keys",
			TOML: `[profile]
name = "warn-keys"
author = "Daniel"

[audit]
enabled = true
extra_audit = 123
`,
		},
		{
			ID:   "prof_warnings_dotted_subtables_leaf_only",
			Name: "warn-dotted",
			TOML: `[profile]
name = "warn-dotted"

[servers.vault]
enabled = true
foo.bar = 1

[servers.vault.sub1.sub2]
leaf = 42

[unknown.top]
deep_leaf = 99
`,
		},
		{
			ID:   "prof_warnings_skills_mode",
			Name: "warn-skills-mode",
			TOML: `[profile]
name = "warn-skills-mode"

[servers.skills]
enabled = true
mode = "full"
`,
		},
		{
			ID:   "prof_warnings_usage_mode",
			Name: "warn-usage-mode",
			TOML: `[profile]
name = "warn-usage-mode"

[servers.usage]
enabled = true
mode = "full"
`,
		},
		{
			ID:   "prof_warnings_foreign_mode",
			Name: "warn-foreign-mode",
			TOML: `[profile]
name = "warn-foreign-mode"

[servers.echo]
enabled = true
command = "/bin/echo"
mode = "full"
`,
		},
		{
			ID:   "prof_warnings_core_access_tools",
			Name: "warn-core-access",
			TOML: `[profile]
name = "warn-core-access"

[servers.vault]
enabled = true
access = "read"
tools_read = ["get_entry"]
tools_write = ["set_entry_field"]
`,
		},
		{
			ID:   "prof_error_name_mismatch",
			Name: "expected-name",
			TOML: `[profile]
name = "actual-different-name"
`,
		},
		{
			ID:   "prof_error_invalid_name_chars",
			Name: "bad/name",
			TOML: `[profile]
name = "bad/name"
`,
		},
		{
			ID:   "prof_error_invalid_name_traversal",
			Name: "../../etc",
			TOML: `[profile]
name = "../../etc"
`,
		},
		{
			ID:   "prof_error_missing_name",
			Name: "",
			TOML: `[profile]
name = ""
`,
		},
		{
			ID:   "prof_error_malformed_toml",
			Name: "broken",
			TOML: `[profile
name = "broken"
`,
		},
		{
			ID:   "prof_error_invalid_vault_mode",
			Name: "bad-vault",
			TOML: `[profile]
name = "bad-vault"

[servers.vault]
enabled = true
mode = "godmode"
`,
		},
		{
			ID:   "prof_error_invalid_memory_mode",
			Name: "bad-memory",
			TOML: `[profile]
name = "bad-memory"

[servers.memory]
enabled = true
mode = "write_only"
`,
		},
		{
			ID:   "prof_error_foreign_missing_transport",
			Name: "bad-foreign",
			TOML: `[profile]
name = "bad-foreign"

[servers.orphan]
enabled = true
`,
		},
		{
			ID:   "prof_error_foreign_invalid_access",
			Name: "bad-access",
			TOML: `[profile]
name = "bad-access"

[servers.custom]
enabled = true
command = "/usr/bin/tool"
access = "admin"
`,
		},
		{
			ID:   "prof_error_core_with_command",
			Name: "bad-core-collision",
			TOML: `[profile]
name = "bad-core-collision"

[servers.vault]
enabled = true
command = "/bin/custom-vault"
`,
		},
	}
	return append(cases, getDecodeErrorProfileTestCases()...)
}
