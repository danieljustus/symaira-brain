"""Additive byte/outer/control-owner plans; never executed by source checks."""
import os
from cases import TARGETS
from fixtures import remove_tree

RAW = {"ff": b"\xff", "e282": b"\xe2\x82", "c0af": b"\xc0\xaf", "literal": "\ufffd".encode()}
BODY = {**RAW, "ordinary": b"ordinary UTF8"}
OUTER = {
    "meta-array-read": b'{"name":"skills_list","arguments":{},"_meta":[]}',
    "meta-array-write": b'{"name":"skills_install","arguments":{"name":"demo","dry_run":false},"_meta":[]}',
    "meta-overflow-read": b'{"name":"skills_list","arguments":{},"_meta":{"token":1e9999}}',
    "name-type-then-valid": b'{"name":4,"name":"skills_list","arguments":{}}',
    "name-valid-then-null": b'{"name":"skills_list","name":null,"arguments":{}}',
    "meta-null-control": b'{"name":"skills_list","arguments":{},"_meta":null}',
    "meta-normal-control": b'{"name":"skills_list","arguments":{},"_meta":{"token":1}}',
    "meta-first-error-retained": b'{"_meta":[],"_meta":{},"name":"skills_list","arguments":{}}',
    "meta-nested-overflow": b'{"name":"skills_list","arguments":{},"_meta":{"nested":[{"token":1e9999}]}}',
    "arguments-last-null": b'{"name":"skills_list","arguments":{},"arguments":null}',
}


def request(params, outer=b""):
    return b'{"jsonrpc":"2.0","id":3,"method":"tools/call",' + outer + b'"params":' + params + b'}'


def transport(bootstrap, body, framed):
    # The original initialization/catalog inputs are copied exactly; only the
    # fourth actual input is replaced, including raw bytes before decoding.
    prefix = bootstrap.splitlines()[:3]
    lines = [*prefix, body]
    return b"".join(b"Content-Length: " + str(len(line)).encode() + b"\r\n\r\n" + line
                    for line in lines) if framed else b"\n".join(lines) + b"\n"


def cases(root, incoming):
    rows = []
    argv = ["mcp", "--profile-file", str(root / "profile.toml")]
    bootstrap = incoming("skills_list", {})
    for name, params in OUTER.items():
        for framed in (False, True):
            rows.append((f"byte-outer-{name}-{'framed' if framed else 'line'}", argv,
                         transport(bootstrap, request(params), framed), None))
    for key, raw in RAW.items():
        for position in ("argument-value", "argument-key"):
            arguments = b'{"ignored":"' + raw + b'"}' if position == "argument-value" else b'{"' + raw + b'":1e9999}'
            params = b'{"name":"skills_list","arguments":' + arguments + b'}'
            for framed in (False, True):
                rows.append((f"byte-transport-{position}-{key}-{'framed' if framed else 'line'}", argv,
                             transport(bootstrap, request(params), framed), None))
    for position in ("params", "request"):
        params = br'{"name":"skills_list","arguments":{}}'
        if position == "params": params = br'{"name":"skills_list","\ud800":1,"arguments":{}}'
        for framed in (False, True):
            body = request(params, br'"\ud800":1,' if position == "request" else b"")
            rows.append((f"byte-transport-surrogate-{position}-{'framed' if framed else 'line'}", argv,
                         transport(bootstrap, body, framed), None))
    native_paths = ["ff", "e282", "c0af", "literal", "js2028", "js2029", "html-js"]
    if os.name == "nt": native_paths[:3] = ["high", "low", "separated"]
    for key in native_paths:
        for form, flags in (("table", []), ("json", ["--json"])):
            rows.append((f"byte-path-doctor-{key}-{form}", ["skills", "doctor", *flags], None, "byte-path-" + key))
        rows.append((f"byte-path-library-{key}", ["skills", "list", "--json"], None, "byte-path-" + key))
        rows.append((f"byte-path-mcp-library-{key}", argv, incoming("skills_list", {}), "byte-path-" + key))
        for form, flags in (("table", []), ("json", ["--json"])):
            rows.append((f"byte-path-targets-{key}-{form}", ["skills", "targets", *flags], None, "byte-home-" + key))
    for key in BODY:
        selected = "byte-body-" + key
        rows.append(("byte-body-cli-list-" + key, ["skills", "list", "--json"], None, selected))
        for tool in ("skills_list", "skills_inspect", "skills_validate"):
            rows.append((f"byte-body-{tool}-{key}", argv, incoming(tool, {} if tool == "skills_list" else {"name": "demo"}), selected))
        for target in TARGETS:
            for tool, dry in (("skills_render_plan", True), ("skills_install", False)):
                rows.append((f"byte-body-{tool}-{target}-{key}", argv, incoming(tool, {"name": "demo", "target": target, "dry_run": dry}), selected))
    for key in ("ff", "e282", "c0af", "ordinary"):
        selected = "byte-original-body-" + key
        rows.append(("byte-original-body-cli-list-" + key, ["skills", "list", "--json"], None, selected))
        rows.append(("byte-original-body-inspect-" + key, argv, incoming("skills_inspect", {"name": "demo"}), selected))
    if os.name != "nt":
        for kind in ("skill-link", "manifest-link", "resource-link"):
            selected = "byte-control-" + kind
            rows.append((f"byte-control-{kind}-cli-list", ["skills", "list", "--json"], None, selected))
            for tool in ("skills_list", "skills_inspect", "skills_validate", "skills_render_plan", "skills_discover_sources"):
                values = {} if tool == "skills_list" else {"paths": [str(root / "sources/control")]} if tool == "skills_discover_sources" else {"name": "demo", "target": "opencode", "dry_run": True}
                rows.append((f"byte-control-{kind}-{tool}", argv, incoming(tool, values), selected))
    return rows


def original_document(key):
    return b"---\nname: demo\ndescription: valid header\n---\nbody-" + BODY[key] + b"\n"


def path_suffix(key):
    if key in RAW: return os.fsdecode(RAW[key])
    return {"high": "\ud800", "low": "\udc00", "separated": "\ud800x\udc00",
            "js2028": "\u2028", "js2029": "\u2029", "html-js": "<>&\u2028\u2029"}[key]


def variant(root, env, name):
    env = dict(env)
    if name.startswith(("byte-path-", "byte-home-")):
        key = name.removeprefix("byte-path-").removeprefix("byte-home-")
        # Raw environment presentation requires no kernel filename admission.
        value = str(root / "missing-byte-path-") + path_suffix(key)
        if name.startswith("byte-path-"): env["SYMSKILLS_LIBRARY_DIR"] = value
        else: env.update(HOME=value, USERPROFILE=value)
        return env
    bundle = root / "data/symbrain/skills/library/demo"
    if name.startswith("byte-original-body-"):
        (bundle / "SKILL.md").write_bytes(original_document(name.removeprefix("byte-original-body-")))
    elif name.startswith("byte-body-"):
        raw = BODY[name.removeprefix("byte-body-")]
        (bundle / "SKILL.md").write_bytes(b"---\r\nname: demo\r\ndescription: valid header\r\n---\r\nbody-" + raw + b"\r\n")
    elif name.startswith("byte-control-"):
        for directory in (bundle, root / "sources/control"):
            directory.mkdir(exist_ok=True)
            doc = directory / "SKILL.md"
            doc.write_bytes(b"---\nname: demo\ndescription: valid header\n---\nbody\n")
            kind = name.removeprefix("byte-control-")
            if kind == "skill-link":
                doc.rename(directory / "document.md")
                os.symlink("document.md", doc)
            elif kind == "manifest-link":
                (directory / "manifest.toml").write_text('[skill]\nname = "demo"\n')
                os.symlink("manifest.toml", directory / "symskills.toml")
            elif kind == "resource-link":
                (directory / "assets").mkdir()
                (directory / "assets/notes.md").write_bytes(b"confined resource\n")
                os.symlink("assets", directory / "linked-assets", target_is_directory=True)
            else: raise ValueError("unknown byte control variant")
    else: raise ValueError("unknown byte variant")
    for path in [root, *root.rglob("*")]:
        if not path.is_symlink(): os.utime(path, ns=(1893456000000000123, 1577934245000000000))
    return env


def input_description(env):
    def raw(value):
        return value.encode("utf-16-le", "surrogatepass").hex() if os.name == "nt" else os.fsencode(value).hex()
    return {"encoding": "native-utf16le" if os.name == "nt" else "native-unix-bytes",
            "path_environment_hex": {key: raw(env[key]) for key in ("HOME", "USERPROFILE", "SYMSKILLS_LIBRARY_DIR") if key in env}}


def run_controls(report, save, binaries, root, retained, env, invoke, incoming, matched, mcp_view):
    import shutil
    argv = ["mcp", "--profile-file", str(root / "profile.toml")]
    mutations = [
        ("byte-outer-meta-control", "byte-outer-meta-normal-control-line", None,
         transport(incoming("skills_list", {}), request(OUTER["meta-array-read"]), False)),
        ("byte-body-admission-control", "byte-body-skills_inspect-ff", "byte-body-ff",
         incoming("skills_inspect", {"name": 4})),
    ]
    for name, selector, selected, data in mutations:
        baseline = next(row for row in report["results"] if row["id"] == selector)
        assert baseline["matched"] and matched(baseline["go"], baseline["rust"], root, True)
        row = {"name": name, "baseline": selector, "stdin_hex": data.hex(), "candidate": {}, "rejected": False}
        report["controls"].append(row)
        save()
        try:
            remove_tree(root)
            shutil.copytree(retained, root, symlinks=True)
            case_env = variant(root, env, selected) if selected else env
            invoke(binaries["rust"], argv, case_env, root, data, record=row["candidate"])
            row["candidate"]["expected_skills"] = 11
            row["rejected"] = not matched(baseline["go"], row["candidate"], root, True)
            save()
            assert row["rejected"] and row["candidate"]["exit"] == 0, "bootstrap failure cannot pass a byte-owner control"
            reply = mcp_view(row["candidate"])[1][2]
            if selected:
                assert reply["result"]["isError"] is True
                assert "parse arguments" in reply["result"]["content"][0]["text"]
            else:
                assert reply["error"]["code"] == -32602 and "._meta" in reply["error"]["message"]
        except BaseException as error:
            row["control_error"] = {"type": type(error).__name__, "message": str(error)}
            raise
        finally:
            save()
