package main

func getDecodeErrorProfileTestCases() []ProfileTestCase {
	return []ProfileTestCase{
		{
			ID:   "prof_error_wrong_profile_type",
			Name: "wrong-profile-type",
			TOML: `profile = 123
`,
		},
		{
			ID:   "prof_error_wrong_profile_name_type",
			Name: "wrong-profile-name",
			TOML: `[profile]
name = 123
`,
		},
		{
			ID:   "prof_error_wrong_profile_description_type",
			Name: "wrong-profile-desc",
			TOML: `[profile]
name = "wrong-profile-desc"
description = 123
`,
		},
		{
			ID:   "prof_error_wrong_audit_type",
			Name: "wrong-audit-type",
			TOML: `audit = "yes"

[profile]
name = "wrong-audit-type"
`,
		},
		{
			ID:   "prof_error_wrong_audit_enabled_type",
			Name: "wrong-audit-enabled",
			TOML: `[profile]
name = "wrong-audit-enabled"

[audit]
enabled = "yes"
`,
		},
		{
			ID:   "prof_error_wrong_server_entry_int",
			Name: "wrong-server-entry-int",
			TOML: `[profile]
name = "wrong-server-entry-int"

[servers]
vault = 123
`,
		},
		{
			ID:   "prof_error_wrong_server_entry_str",
			Name: "wrong-server-entry-str",
			TOML: `[profile]
name = "wrong-server-entry-str"

[servers]
vault = "bad"
`,
		},
		{
			ID:   "prof_error_wrong_server_entry_arr",
			Name: "wrong-server-entry-arr",
			TOML: `[profile]
name = "wrong-server-entry-arr"

[servers]
vault = [1, 2]
`,
		},
		{
			ID:   "prof_error_wrong_server_inline_int",
			Name: "wrong-server-inline-int",
			TOML: `servers = { vault = 123 }

[profile]
name = "wrong-server-inline-int"
`,
		},
		{
			ID:   "prof_error_wrong_server_inline_str",
			Name: "wrong-server-inline-str",
			TOML: `servers = { vault = "bad" }

[profile]
name = "wrong-server-inline-str"
`,
		},
		{
			ID:   "prof_error_wrong_server_enabled_type",
			Name: "wrong-server-enabled",
			TOML: `[profile]
name = "wrong-server-enabled"

[servers.vault]
enabled = "true"
`,
		},
		{
			ID:   "prof_error_wrong_server_mode_type",
			Name: "wrong-server-mode",
			TOML: `[profile]
name = "wrong-server-mode"

[servers.vault]
mode = 123
`,
		},
		{
			ID:   "prof_error_wrong_command_type",
			Name: "wrong-server-cmd",
			TOML: `[profile]
name = "wrong-server-cmd"

[servers.echo]
command = 123
`,
		},
		{
			ID:   "prof_error_wrong_url_type",
			Name: "wrong-server-url",
			TOML: `[profile]
name = "wrong-server-url"

[servers.fig]
url = true
`,
		},
		{
			ID:   "prof_error_wrong_access_type",
			Name: "wrong-server-access",
			TOML: `[profile]
name = "wrong-server-access"

[servers.echo]
command = "/bin/echo"
access = 123
`,
		},
		{
			ID:   "prof_error_wrong_tools_allow_type",
			Name: "wrong-tools-allow-type",
			TOML: `[profile]
name = "wrong-tools-allow-type"

[servers.vault]
tools_allow = "all"
`,
		},
		{
			ID:   "prof_error_wrong_tools_allow_elem",
			Name: "wrong-tools-allow-elem",
			TOML: `[profile]
name = "wrong-tools-allow-elem"

[servers.vault]
tools_allow = [123]
`,
		},
		{
			ID:   "prof_error_wrong_tools_deny_type",
			Name: "wrong-tools-deny-type",
			TOML: `[profile]
name = "wrong-tools-deny-type"

[servers.vault]
tools_deny = 123
`,
		},
		{
			ID:   "prof_error_wrong_tools_deny_elem",
			Name: "wrong-tools-deny-elem",
			TOML: `[profile]
name = "wrong-tools-deny-elem"

[servers.vault]
tools_deny = [true]
`,
		},
		{
			ID:   "prof_error_wrong_args_type",
			Name: "wrong-args-type",
			TOML: `[profile]
name = "wrong-args-type"

[servers.echo]
command = "/bin/echo"
args = "foo"
`,
		},
		{
			ID:   "prof_error_wrong_args_elem",
			Name: "wrong-args-elem",
			TOML: `[profile]
name = "wrong-args-elem"

[servers.echo]
command = "/bin/echo"
args = [123]
`,
		},
		{
			ID:   "prof_error_wrong_tools_read_type",
			Name: "wrong-tools-read-type",
			TOML: `[profile]
name = "wrong-tools-read-type"

[servers.echo]
command = "/bin/echo"
tools_read = "read"
`,
		},
		{
			ID:   "prof_error_wrong_tools_read_elem",
			Name: "wrong-tools-read-elem",
			TOML: `[profile]
name = "wrong-tools-read-elem"

[servers.echo]
command = "/bin/echo"
tools_read = [123]
`,
		},
		{
			ID:   "prof_error_wrong_tools_write_type",
			Name: "wrong-tools-write-type",
			TOML: `[profile]
name = "wrong-tools-write-type"

[servers.echo]
command = "/bin/echo"
tools_write = "write"
`,
		},
		{
			ID:   "prof_error_wrong_tools_write_elem",
			Name: "wrong-tools-write-elem",
			TOML: `[profile]
name = "wrong-tools-write-elem"

[servers.echo]
command = "/bin/echo"
tools_write = [false]
`,
		},
	}
}
