//! Unit tests asserting that invalid TOML types fail with [`ProfileError::ParseFailed`].

use symbrain_policy::error::ProfileError;
use symbrain_policy::profile::parse::parse;

fn assert_parse_failed(name: &str, toml: &str) {
    let err = parse(name, toml).expect_err(&format!("expected parse failure for {name}"));
    assert!(
        matches!(err, ProfileError::ParseFailed { .. }),
        "expected ParseFailed for {name}, got: {err:?}"
    );
    let msg = err.to_string();
    assert!(
        msg.contains("failed to parse TOML"),
        "error message should contain 'failed to parse TOML', got: {msg}"
    );
}

#[test]
fn test_wrong_profile_type() {
    assert_parse_failed("wrong-type", "profile = 123\n");
}

#[test]
fn test_wrong_profile_name_type() {
    assert_parse_failed("wrong-name", "[profile]\nname = 123\n");
}

#[test]
fn test_wrong_profile_description_type() {
    assert_parse_failed(
        "wrong-desc",
        "[profile]\nname = \"wrong-desc\"\ndescription = 123\n",
    );
}

#[test]
fn test_wrong_audit_type() {
    assert_parse_failed(
        "wrong-audit",
        "audit = \"yes\"\n[profile]\nname = \"wrong-audit\"\n",
    );
}

#[test]
fn test_wrong_audit_enabled_type() {
    assert_parse_failed(
        "wrong-enabled",
        "[profile]\nname = \"wrong-enabled\"\n[audit]\nenabled = \"yes\"\n",
    );
}

#[test]
fn test_wrong_servers_type() {
    assert_parse_failed(
        "wrong-servers",
        "servers = \"foo\"\n[profile]\nname = \"wrong-servers\"\n",
    );
}

#[test]
fn test_wrong_server_entry_integer() {
    assert_parse_failed(
        "wrong-entry-int",
        "[profile]\nname = \"wrong-entry-int\"\n[servers]\nvault = 123\n",
    );
}

#[test]
fn test_wrong_server_entry_string() {
    assert_parse_failed(
        "wrong-entry-str",
        "[profile]\nname = \"wrong-entry-str\"\n[servers]\nvault = \"bad\"\n",
    );
}

#[test]
fn test_wrong_server_entry_array() {
    assert_parse_failed(
        "wrong-entry-arr",
        "[profile]\nname = \"wrong-entry-arr\"\n[servers]\nvault = [1, 2]\n",
    );
}

#[test]
fn test_wrong_server_enabled_boolean_mismatch() {
    assert_parse_failed(
        "wrong-enabled",
        "[profile]\nname = \"wrong-enabled\"\n[servers.vault]\nenabled = \"true\"\n",
    );
}

#[test]
fn test_wrong_server_mode_string_mismatch() {
    assert_parse_failed(
        "wrong-mode",
        "[profile]\nname = \"wrong-mode\"\n[servers.vault]\nmode = 123\n",
    );
}

#[test]
fn test_wrong_command_string_mismatch() {
    assert_parse_failed(
        "wrong-cmd",
        "[profile]\nname = \"wrong-cmd\"\n[servers.echo]\ncommand = 123\n",
    );
}

#[test]
fn test_wrong_url_string_mismatch() {
    assert_parse_failed(
        "wrong-url",
        "[profile]\nname = \"wrong-url\"\n[servers.fig]\nurl = true\n",
    );
}

#[test]
fn test_wrong_access_string_mismatch() {
    assert_parse_failed(
        "wrong-access",
        "[profile]\nname = \"wrong-access\"\n[servers.echo]\ncommand = \"/bin/echo\"\naccess = 123\n",
    );
}

#[test]
fn test_wrong_tools_allow_not_array() {
    assert_parse_failed(
        "wrong-allow",
        "[profile]\nname = \"wrong-allow\"\n[servers.vault]\ntools_allow = \"all\"\n",
    );
}

#[test]
fn test_wrong_tools_allow_non_string_element() {
    assert_parse_failed(
        "wrong-allow-elem",
        "[profile]\nname = \"wrong-allow-elem\"\n[servers.vault]\ntools_allow = [123]\n",
    );
}

#[test]
fn test_wrong_tools_deny_not_array() {
    assert_parse_failed(
        "wrong-deny",
        "[profile]\nname = \"wrong-deny\"\n[servers.vault]\ntools_deny = 123\n",
    );
}

#[test]
fn test_wrong_tools_deny_non_string_element() {
    assert_parse_failed(
        "wrong-deny-elem",
        "[profile]\nname = \"wrong-deny-elem\"\n[servers.vault]\ntools_deny = [true]\n",
    );
}

#[test]
fn test_wrong_args_not_array() {
    assert_parse_failed(
        "wrong-args",
        "[profile]\nname = \"wrong-args\"\n[servers.echo]\ncommand = \"/bin/echo\"\nargs = \"foo\"\n",
    );
}

#[test]
fn test_wrong_args_non_string_element() {
    assert_parse_failed(
        "wrong-args-elem",
        "[profile]\nname = \"wrong-args-elem\"\n[servers.echo]\ncommand = \"/bin/echo\"\nargs = [123]\n",
    );
}

#[test]
fn test_wrong_tools_read_not_array() {
    assert_parse_failed(
        "wrong-read",
        "[profile]\nname = \"wrong-read\"\n[servers.echo]\ncommand = \"/bin/echo\"\ntools_read = \"read\"\n",
    );
}

#[test]
fn test_wrong_tools_read_non_string_element() {
    assert_parse_failed(
        "wrong-read-elem",
        "[profile]\nname = \"wrong-read-elem\"\n[servers.echo]\ncommand = \"/bin/echo\"\ntools_read = [123]\n",
    );
}

#[test]
fn test_wrong_tools_write_not_array() {
    assert_parse_failed(
        "wrong-write",
        "[profile]\nname = \"wrong-write\"\n[servers.echo]\ncommand = \"/bin/echo\"\ntools_write = \"write\"\n",
    );
}

#[test]
fn test_wrong_tools_write_non_string_element() {
    assert_parse_failed(
        "wrong-write-elem",
        "[profile]\nname = \"wrong-write-elem\"\n[servers.echo]\ncommand = \"/bin/echo\"\ntools_write = [false]\n",
    );
}
