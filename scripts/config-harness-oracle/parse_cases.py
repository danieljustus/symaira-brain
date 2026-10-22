"""Harness config parse-error corpus for `harness list` (#769 slice 2, item D1).

Every case writes one or more broken (or borderline) harness configs into the
disposable per-case HOME/XDG fixture and replays `harness list` in table and
`--json` form against the pinned Go CLI. Families follow the Go error sources:

* JSON: `decodeJSONObject` in `internal/harness/ordered_map.go`
  (`encoding/json` `Decoder.Token`/`Decode`, wrapped as `parse json: ...`);
* TOML: BurntSushi/toml v1.6.0 `toml.Unmarshal` (`parse toml: toml: line N ...`);
* I/O: `readConfigFile`/`openConfigFile` (`os.PathError`, `syscall.Errno` text,
  the 8 MiB limit, non-regular files, symlinked components).

The corpus data is only bytes plus a harness name; Go decides every expected
message, so nothing here duplicates the Rust implementation.
"""
from __future__ import annotations

import os
import platform
from pathlib import Path

# (family, harness, config bytes)
JSON_CASES: list[tuple[str, str, bytes]] = [
    ("empty", "claude", b""),
    ("blank-ascii", "claude", b" \n\t\r"),
    ("blank-unicode", "claude", "\u00a0\u2003\u3000".encode()),
    ("blank-formfeed", "claude", b"\x0c\x0b"),
    ("eof-open", "claude", b"{"),
    ("eof-key", "claude", b'{"a"'),
    ("eof-colon", "claude", b'{"a":'),
    ("eof-value", "claude", b'{"a":1'),
    ("eof-comma", "claude", b'{"a":1,'),
    ("eof-array", "claude", b'{"a":[1,2'),
    ("unexpected-eof-string", "claude", b'{"a":"abc'),
    ("unexpected-eof-escape", "claude", b'{"a":"\\'),
    ("unexpected-eof-unicode", "claude", b'{"a":"\\u12'),
    ("unexpected-eof-literal", "claude", b'{"a":tru'),
    ("unexpected-eof-number", "claude", b'{"a":-'),
    ("unexpected-eof-exp", "claude", b'{"a":1e'),
    ("top-array", "claude", b"[1]"),
    ("top-array-open", "claude", b"["),
    ("top-string", "claude", b'"x"'),
    ("top-string-escapes", "claude", b'"a\\n\\u00e9\\ud83d\\ude00\\ud800z"'),
    ("top-string-specials", "claude", '"\\b\\f\\u0001<>&\u2028\u2029"'.encode()),
    ("top-number", "claude", b"-1.5e3"),
    ("top-true", "claude", b"true"),
    ("top-false", "claude", b"false"),
    ("top-null", "claude", b"null"),
    ("top-close-brace", "claude", b"}"),
    ("top-close-bracket", "claude", b"]"),
    ("top-colon", "claude", b":"),
    ("top-comma", "claude", b","),
    ("top-bom", "claude", b"\xef\xbb\xbf{}"),
    ("top-utf16-bom", "claude", b"\xff\xfe{\x00}\x00"),
    ("top-garbage", "claude", b"\xff{}"),
    ("not-literal", "claude", b"{not-json"),
    ("key-bare", "claude", b"{a:1}"),
    ("key-single-quote", "claude", b"{'a':1}"),
    ("key-bracket", "claude", b"{]"),
    ("key-open-brace", "claude", b"{{}}"),
    ("key-number", "claude", b"{1:2}"),
    ("after-key", "claude", b'{"a" 1}'),
    ("after-key-close", "claude", b'{"a"}'),
    ("double-colon", "claude", b'{"a"::1}'),
    ("value-missing", "claude", b'{"a":}'),
    ("value-bracket", "claude", b'{"a":]}'),
    ("value-comma", "claude", b'{"a":,}'),
    ("pair-missing-comma", "claude", b'{"a":1 "b":2}'),
    ("trailing-comma-object", "claude", b'{"a":1,}'),
    ("leading-comma-object", "claude", b"{,}"),
    ("double-comma", "claude", b'{"a":1,,"b":2}'),
    ("array-missing-comma", "claude", b'{"a":[1 2]}'),
    ("array-trailing-comma", "claude", b'{"a":[1,]}'),
    ("array-mismatch", "claude", b'{"a":[1}'),
    ("object-mismatch", "claude", b'{"a":{"b":1]}'),
    ("trailing-text", "claude", b"{} x"),
    ("trailing-object", "claude", b"{}{}"),
    ("trailing-nul", "claude", b'{"a":1}\x00'),
    ("trailing-close-bracket", "claude", b"{}]"),
    ("trailing-close-brace", "claude", b"{}}"),
    ("literal-true", "claude", b'{"a":tru}'),
    ("literal-false", "claude", b'{"a":falsx}'),
    ("literal-null", "claude", b'{"a":nul}'),
    ("literal-overlong", "claude", b'{"a":truee}'),
    ("number-plus", "claude", b'{"a":+1}'),
    ("number-dot", "claude", b'{"a":.5}'),
    ("number-minus", "claude", b'{"a":-}'),
    ("number-minus-x", "claude", b'{"a":-x}'),
    ("number-fraction", "claude", b'{"a":1.}'),
    ("number-fraction-e", "claude", b'{"a":-0.}'),
    ("number-exponent", "claude", b'{"a":1.5e}'),
    ("number-exponent-sign", "claude", b'{"a":1.5e+}'),
    ("number-leading-zero", "claude", b'{"a":01}'),
    ("number-huge-ok", "claude", b'{"a":123456789012345678901234567890,"b":1e999}'),
    ("string-control", "claude", b'{"a":"\x01"}'),
    ("string-newline", "claude", b'{"a":"x\ny"}'),
    ("string-escape", "claude", b'{"a":"\\x"}'),
    ("string-escape-quote", "claude", b"{\"a\":\"\\'\"}"),
    ("string-unicode-hex", "claude", b'{"a":"\\u12G4"}'),
    ("string-unicode-short", "claude", b'{"a":"\\u12"}'),
    ("string-invalid-utf8-ok", "claude", b'{"mcpServers":{"\xff":{"command":"\xc3"}}}'),
    ("string-surrogates-ok", "claude", b'{"mcpServers":{"\\ud800":{"command":"\\ud83d\\ude00\\udc00"}}}'),
    ("char-control", "claude", b"{\x01}"),
    ("char-del", "claude", b"{\x7f}"),
    ("char-c1", "claude", b'{"a":\x80}'),
    ("char-nbsp", "claude", b'{"a":\xc2\xa0}'),
    ("char-shy", "claude", b'{"a":\xc2\xad}'),
    ("char-latin1", "claude", b'{"a":\xc3\xa9}'),
    ("char-html", "claude", b'{"a":<>&}'),
    ("char-quote", "claude", b'{"a":"b"c}'),
    ("char-backslash", "claude", b'{"a":\\}'),
    ("char-line-separator", "claude", "{\"a\":\u2028}".encode()),
    ("char-newline", "claude", b'{"a":\n}'),
    ("char-tab", "claude", b'{"a":1\t\x08}'),
    ("deep-ok", "claude", b'{"a":' + b"[" * 20000 + b"]" * 20000 + b"}"),
    ("deep-eof", "claude", b'{"a":' + b"[" * 20000),
    ("deep-mismatch", "claude", b'{"a":' + b"[" * 12000 + b"}" * 12000 + b"}"),
    ("valid-servers", "claude", b'{"mcpServers":{"b":{"type":"http","url":"http://h"},"a":{"command":"c","args":["x",1,"y"],"env":{"K":"v"}}}}'),
    ("valid-duplicate-keys", "claude", b'{"mcpServers":{"a":{"command":"1"},"a":{"command":"2"}},"mcpServers":{"z":{}}}'),
    ("claude-desktop-eof", "claude-desktop", b"{"),
    ("claude-desktop-array", "claude-desktop", b"[]"),
    ("claude-desktop-literal", "claude-desktop", b'{"a":tru}'),
    ("cursor-eof", "cursor", b"{"),
    ("cursor-trailing", "cursor", b'{"mcpServers":{}} x'),
    ("cursor-utf8", "cursor", b'{"mcpServers":{"\xc3\xa9":{"command":"\xe2\x82\xac"}}}\xff'),
    ("opencode-eof", "opencode", b"{"),
    ("opencode-key", "opencode", b"{a:1}"),
    ("opencode-empty", "opencode", b""),
    ("antigravity-eof", "antigravity", b"{"),
    ("antigravity-array", "antigravity", b"[1]"),
    ("antigravity-empty", "antigravity", b" \n"),
]

TOML_CASES: list[tuple[str, bytes]] = [
    ("empty", b""),
    ("newlines", b"\n\n"),
    ("comment", b"# c\n"),
    ("bom-ok", b"\xef\xbb\xbfa = 1"),
    ("bom-garbage", b"\xef\xbb\xbfa = "),
    ("utf16-bom", b"\xff\xfea = 1"),
    ("utf16-bom-be", b"\xfe\xffa"),
    ("nul-first", b"\x00"),
    ("nul-early", b"ab\x00"),
    ("nul-late", b"a = 1\n\x00"),
    ("key-eof", b"a"),
    ("key-eof-value", b"a ="),
    ("key-space", b"a b = 1"),
    ("key-blank", b"= 1"),
    ("key-dot-start", b".a = 1"),
    ("key-dot-end", b"a. = 1"),
    ("key-dot-dot", b"a..b = 1"),
    ("key-quote-eof", b'a."b = 1'),
    ("key-quoted-ok", b'"a b".c = 1'),
    ("key-quoted-newline", b'"a\nb" = 1'),
    ("key-quoted-escape", b'"a\\q" = 1'),
    ("key-after-quote", b'"a"b = 1'),
    ("value-eof", b"a = "),
    ("value-bare", b"a = x"),
    ("value-bare-long", b"a = foo bar"),
    ("value-underscore", b"a = _1"),
    ("value-dot", b"a = .5"),
    ("value-unicode-letter", "a = \u03b1\u03b2".encode()),
    ("value-unicode-number", "a = \u2167".encode()),
    ("value-nbsp", b"a = \xc2\xa0"),
    ("value-html", b"a = <"),
    ("value-newline", b"a =\n1"),
    ("value-trailing", b"a = true false"),
    ("value-two-ints", b"a = 1 2"),
    ("top-end-quote", b"a = 1 '"),
    ("string-eof", b'a = "x'),
    ("string-newline", b'a = "x\n"'),
    ("string-raw-eof", b"a = 'x"),
    ("string-raw-newline", b"a = 'x\n'"),
    ("string-escape", b'a = "\\q"'),
    ("string-escape-space", b'a = "\\ "'),
    ("string-escape-surrogate", b'a = "\\ud800"'),
    ("string-escape-range", b'a = "\\U00110000"'),
    ("string-escape-x-short", b'a = "\\x4"'),
    ("string-escape-u-bad", b'a = "\\u12G4"'),
    ("string-escape-U-short", b'a = "\\U1234"'),
    ("string-escape-ok", b'a = "\\e\\x41\\u00e9\\U0001F600"'),
    ("multi-eof", b'a = """x'),
    ("multi-raw-eof", b"a = '''x"),
    ("multi-six-quotes", b'a = """x""""""'),
    ("multi-raw-six-quotes", b"a = '''x''''''"),
    ("multi-bad-escape", b'a = """x\\ y"""'),
    ("multi-line-ending-ok", b'a = """x\\\n   y"""'),
    ("array-eof", b"a = [1,"),
    ("array-missing-comma", b"a = [1 2]"),
    ("array-leading-comma", b"a = [,1]"),
    ("array-double-comma", b"a = [1,,2]"),
    ("array-unclosed-nested", b"a = [[1],[2]"),
    ("array-junk", b"a = [1] x"),
    ("array-bad-element", b"a = [1, x]"),
    ("inline-eof", b"a = {"),
    ("inline-eof-pair", b"a = {b=1"),
    ("inline-missing-comma", b"a = {b=1 c=2}"),
    ("inline-trailing-comma", b"a = {b=1,}"),
    ("inline-leading-comma", b"a = {,}"),
    ("inline-newline", b"a = {\n b=1\n}"),
    ("inline-duplicate", b"a = {b=1, b=2}"),
    ("inline-conflict", b"a = {b=1, b.c=2}"),
    ("table-eof", b"["),
    ("table-name-eof", b"[a"),
    ("table-empty", b"[]"),
    ("table-dot-start", b"[.a]"),
    ("table-dot-end", b"[a.]"),
    ("table-space", b"[a b]"),
    ("table-extra-close", b"[a]]"),
    ("table-trailing", b"[a] x"),
    ("table-duplicate", b"[a]\n[a]"),
    ("table-after-key", b"a=1\n[a]"),
    ("table-after-dotted", b"[a.b.c]\n[a]\nb.c.d=1"),
    ("table-implicit-twice", b"[a.b]\n[a]\n[a.b]"),
    ("table-hash-conflict", b"[a]\nb = 1\n[a.b.c]"),
    ("table-of-scalar", b"a = 1\n[a.b]"),
    ("array-table-eof", b"[[a]"),
    ("array-table-unclosed", b"[[a"),
    ("array-table-mismatch", b"[[a]x]"),
    ("array-table-extra", b"[[a]]]"),
    ("array-table-over-table", b"[a]\n[[a]]"),
    ("array-table-over-array", b"a = []\n[[a]]"),
    ("array-table-over-inline", b"a = [{b=1}]\n[[a]]"),
    ("table-over-array-table", b"[[a]]\n[a]"),
    ("key-duplicate", b"a=1\na=2"),
    ("key-dotted-conflict", b"a.b=1\na.b.c=2"),
    ("key-inline-extend", b"a={b=1}\n[a.c]"),
    ("key-inline-redefine", b"a={b=1}\n[a]"),
    ("key-dotted-over-inline", b"a={b=1}\na.c=2"),
    ("int-leading-zero", b"a = 01"),
    ("int-underscore-end", b"a = 1_"),
    ("int-underscore-double", b"a = 1__0"),
    ("int-range-high", b"a = 9223372036854775808"),
    ("int-range-low", b"a = -9223372036854775809"),
    ("int-min-ok", b"a = -9223372036854775808"),
    ("int-hex-range", b"a = 0xFFFFFFFFFFFFFFFF"),
    ("int-hex-empty", b"a = 0x"),
    ("int-hex-bad", b"a = 0xg"),
    ("int-hex-underscore", b"a = 0x_1"),
    ("int-binary-bad", b"a = 0b2"),
    ("int-octal-bad", b"a = 0o8"),
    ("int-sign-base", b"a = +0x1"),
    ("float-dot-end", b"a = 1."),
    ("float-dot-exp", b"a = 1.e2"),
    ("float-leading-zero", b"a = 01.5"),
    ("float-two-dots", b"a = 1.5.5"),
    ("float-exponent-end", b"a = 1e"),
    ("float-exponent-sign-end", b"a = 1e+"),
    ("float-range", b"a = 1e999"),
    ("float-underscore", b"a = 1._5"),
    ("float-ok", b"a = [1e5_0, 1_000.5_5, +inf, -inf, nan, +nan, -nan, 6.626e-34]"),
    ("float-ian", b"a = ian"),
    ("float-nnf", b"a = nnf"),
    ("float-in", b"a = in"),
    ("float-i", b"a = i"),
    ("float-sign-i", b"a = +i"),
    ("float-sign-n", b"a = -n"),
    ("float-sign-eof", b"a = +"),
    ("float-sign-x", b"a = +x"),
    ("date-ok", b"a = [1979-05-27, 07:32:00.5, 1979-05-27T07:32:00Z, 1979-05-27 07:32:00+01:30, 1979-05-27t07:32z]"),
    ("date-short-month", b"a = 1979-5-27"),
    ("date-short-hour", b"a = 1979-05-27T7:32:00"),
    ("date-month-13", b"a = 1979-13-27"),
    ("date-feb-30", b"a = 1979-02-30"),
    ("date-feb-29-leap", b"a = 2000-02-29"),
    ("date-feb-29-century", b"a = 1900-02-29"),
    ("date-hour-24", b"a = 24:00:00"),
    ("date-minute-60", b"a = 07:60"),
    ("date-short-time", b"a = 7:32:00"),
    ("date-fraction-dot", b"a = 07:32:00."),
    ("date-offset-25", b"a = 1979-05-27T07:32:00+25:00"),
    ("date-offset-minute-61", b"a = 1979-05-27T07:32:00+00:61"),
    ("date-offset-minus-eof", b"a = 1979-05-27T07:32:00-"),
    ("date-extra", b"a = 1979-05-27x"),
    ("date-dash", b"a = 1-2"),
    ("date-colon", b"a = 12:"),
    ("control-value", b"a = \x01"),
    ("control-string", b'a = "\x01"'),
    ("control-comment", b"# \x01"),
    ("control-del", b"a = \x7f"),
    ("control-bare-cr", b"a = 1\rb = 2"),
    ("control-cr-eof", b"a = 1\r"),
    ("utf8-value", b"a = \xff"),
    ("utf8-comment", b"# \xff"),
    ("utf8-truncated", b'a = "\xc3"'),
    ("utf8-key", b"\xff = 1"),
    ("utf8-continuation", b"a = \x80"),
    ("key-unicode-ok", "\u00e9 = 1".encode()),
    ("key-unicode-bare", b"a \xc3\xa9= 1"),
    ("key-html", b"a<b = 1"),
    ("duplicate-quoted-specials", b'"\\b\\f<>&\\u2028" = 1\n"\\b\\f<>&\\u2028" = 2'),
    ("duplicate-quoted-unicode", "[\"\u2028\U0001f600 \u00a0 \u00ad \u200b\"]\n[\"\u2028\U0001f600 \u00a0 \u00ad \u200b\"]".encode()),
    ("duplicate-quoted-control", b'["\\u0001\\u007f"]\n["\\u0001\\u007f"]'),
    ("valid-servers", b'[mcp_servers.x]\ncommand = "c"\nargs = ["a", 1, "b"]\n[mcp_servers.x.env]\nK = "v"\n[mcp_servers.b]\nurl = "http://h"\ntype = "sse"\n'),
    ("valid-inline-servers", b'mcp_servers = { b = { url = "u" }, a = { command = "c", args = ["x"], env = { K = "v" } } }'),
    ("valid-array-of-tables-args", b'[mcp_servers.x]\ncommand = "c"\n[[mcp_servers.x.args]]\na = 1\n'),
    ("valid-array-of-inline-args", b'[mcp_servers.x]\nargs = [{a=1}, "s"]\n'),
    ("valid-scalar-servers", b"mcp_servers = 1"),
    ("valid-array-servers", b"mcp_servers = []"),
    ("valid-mixed-servers", b'[mcp_servers]\nx = 1\n[mcp_servers.z]\nurl = "http://h"\n'),
]

# Several broken configs at once: registry order, table and JSON rendering.
MULTI_CASES: list[tuple[str, dict[str, bytes], bytes | None]] = [
    ("all-broken", {
        "claude": b"{", "claude-desktop": b"{x", "cursor": b"[]", "opencode": b'{"a":tru}',
        "codex": b"a = ", "antigravity": b'{"a":1,}',
    }, None),
    ("mixed-valid-first", {
        "claude": b'{"mcpServers":{"s":{"command":"c"}}}', "cursor": b"{", "codex": b"[a]\n[a]",
        "antigravity": b"",
    }, None),
    ("mixed-valid-last", {
        "claude": b"{", "claude-desktop": b"[1]", "codex": b'[mcp_servers.x]\ncommand = "c"',
        "opencode": b'{"mcpServers":{"o":{"url":"u"}}}',
    }, None),
    ("project-broken", {"claude": b'{"mcpServers":{"g":{"command":"c"}}}', "codex": b"a = x"}, b'{"mcpServers":{"p":{"command":"p"}'),
    ("project-and-global-broken", {"claude": b"{", "codex": b"a = x"}, b"[]"),
]


def harness_path(env: dict[str, str], name: str) -> Path:
    home = Path(env["HOME"])
    xdg = Path(env["XDG_CONFIG_HOME"])
    if name == "claude":
        return home / ".claude.json"
    if name == "claude-desktop":
        system = platform.system()
        if system == "Darwin":
            return home / "Library" / "Application Support" / "Claude" / "claude_desktop_config.json"
        if system == "Windows":
            return home / "AppData" / "Roaming" / "Claude" / "claude_desktop_config.json"
        return xdg / "Claude" / "claude_desktop_config.json"
    if name == "cursor":
        return home / ".cursor" / "mcp.json"
    if name == "opencode":
        return xdg / "opencode" / "config.json"
    if name == "codex":
        return home / ".codex" / "config.toml"
    if name == "antigravity":
        return home / ".gemini" / "config" / "mcp_config.json"
    raise KeyError(name)


def write_config(env: dict[str, str], name: str, data: bytes) -> Path:
    path = harness_path(env, name)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    return path


def single_setup(name: str, data: bytes):
    def setup(_root: Path, env: dict[str, str]) -> None:
        write_config(env, name, data)
    return setup


def multi_setup(files: dict[str, bytes], project: bytes | None):
    def setup(root: Path, env: dict[str, str]) -> None:
        for name, data in files.items():
            write_config(env, name, data)
        if project is not None:
            (root / "project" / ".mcp.json").write_bytes(project)
    return setup


# POSIX-only I/O families; each returns the harness whose config is affected.
def _io_directory(root, env):
    harness_path(env, "claude").mkdir(parents=True)


def _io_codex_directory(root, env):
    harness_path(env, "codex").mkdir(parents=True)


def _io_symlink(root, env):
    (root / "real.json").write_text("{}")
    harness_path(env, "claude").symlink_to(root / "real.json")


def _io_dangling_symlink(root, env):
    harness_path(env, "claude").symlink_to(root / "nothere.json")


def _io_codex_symlink(root, env):
    (root / "real.toml").write_text("a = 1")
    path = harness_path(env, "codex")
    path.parent.mkdir(parents=True)
    path.symlink_to(root / "real.toml")


def _io_fifo(root, env):
    os.mkfifo(harness_path(env, "claude"))


def _io_unreadable(root, env):
    path = write_config(env, "claude", b"{}")
    path.chmod(0)


def _io_too_large(root, env):
    write_config(env, "claude", b'{"a":"' + b"x" * (8 * 1024 * 1024) + b'"}')


def _io_exact_limit(root, env):
    pad = 8 * 1024 * 1024 - len(b'{"a":""}')
    write_config(env, "claude", b'{"a":"' + b"x" * pad + b'"}')


def _io_parent_file(root, env):
    (Path(env["HOME"]) / ".cursor").write_text("x")


def _io_parent_symlink(root, env):
    (root / "elsewhere").mkdir()
    (root / "elsewhere" / "mcp.json").write_text("{}")
    (Path(env["HOME"]) / ".cursor").symlink_to(root / "elsewhere")


def _io_parent_unreadable(root, env):
    write_config(env, "cursor", b"{}")
    (Path(env["HOME"]) / ".cursor").chmod(0)


def _io_missing_parent_ok(root, env):
    return None


def _io_xdg_symlink(root, env):
    (root / "real-xdg").mkdir()
    link = Path(env["XDG_CONFIG_HOME"])
    if link.exists():
        link.rmdir()
    link.symlink_to(root / "real-xdg")


def _io_project_symlink(root, env):
    (root / "u.json").write_text("{}")
    (root / "project" / ".mcp.json").symlink_to(root / "u.json")


def _io_project_directory(root, env):
    (root / "project" / ".mcp.json").mkdir()


def _io_project_unreadable(root, env):
    path = root / "project" / ".mcp.json"
    path.write_text("{}")
    path.chmod(0)


def _io_project_root_symlink(root, env):
    (root / "project" / "alink").symlink_to(".")


def _io_project_root_file(root, env):
    (root / "project" / "afile").write_text("x")


IO_CASES: list[tuple[str, object, tuple[str, ...]]] = [
    ("directory", _io_directory, ()),
    ("codex-directory", _io_codex_directory, ()),
    ("symlink", _io_symlink, ()),
    ("dangling-symlink", _io_dangling_symlink, ()),
    ("codex-symlink", _io_codex_symlink, ()),
    ("fifo", _io_fifo, ()),
    ("unreadable", _io_unreadable, ()),
    ("too-large", _io_too_large, ()),
    ("exact-limit", _io_exact_limit, ()),
    ("parent-file", _io_parent_file, ()),
    ("parent-symlink", _io_parent_symlink, ()),
    ("parent-unreadable", _io_parent_unreadable, ()),
    ("missing-parent", _io_missing_parent_ok, ()),
    ("xdg-symlink", _io_xdg_symlink, ()),
    ("project-symlink", _io_project_symlink, ("--project", "PROJECT")),
    ("project-symlink-relative", _io_project_symlink, ("--project", ".")),
    ("project-directory", _io_project_directory, ("--project", "PROJECT")),
    ("project-unreadable", _io_project_unreadable, ("--project", "PROJECT")),
    ("project-root-symlink", _io_project_root_symlink, ("--project", "alink")),
    ("project-root-file", _io_project_root_file, ("--project", "afile")),
    ("project-parent-component", _io_missing_parent_ok, ("--project", "../project")),
]
