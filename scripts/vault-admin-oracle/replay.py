#!/usr/bin/env python3
"""Generate real process contracts from an immutable Go archive and replay Rust."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile


def cases():
    result = []

    def add(name, args, value=b"fixture-secret\n", metadata=None, **extra):
        result.append(dict(name=name, args=args, input_hex=value.hex(),
                           metadata=metadata, **extra))

    for action in ("create", "set", "delete"):
        for args in ([], ["a", "b", "c"], ["--help"]):
            add(f"{action}-arity-{len(result)}", [action, *args], b"")
    for path in ("", "/absolute", "trailing/", "a//b", "a/../b", "a\x01b"):
        add(f"invalid-create-{len(result)}", ["create", path])
    for path in ("a", ".password", "a.", "a/../b.password", "a.bad\x01field"):
        add(f"invalid-set-{len(result)}", ["set", path])
    for action, query in (("create", "work/item"), ("set", "work/item.password")):
        for value in (b"", b" \t", "\u00a0\u3000".encode(), b"one\ntwo\n", b"one\rtwo", b"one\n\n"):
            add(f"{action}-invalid-stdin-{len(result)}", [action, query], value)
        for value in (b"fixture-secret", b"fixture-secret\r\n", b" fixture-secret \n"):
            secret = value.rstrip(b"\r\n").decode()
            add(f"{action}-stdin-{len(result)}", [action, query], value,
                json.dumps({"path": "work/item", "fields": {"password": secret}}))
        for value in (b"\xff\n", b"\x1bfixture-secret\n"):
            add(f"{action}-raw-stdin-{len(result)}", [action, query], value,
                '{"fields":{"password":"fixture-secret"}}')
        for metadata in ("{}", "null", "{bad", "[]", '{"path":4}',
                         '{"fields":[]}', '{"path":null,"fields":null}',
                         '{"path":"","fields":{"password":"fixture-secret"}}',
                         '{"path":"confirmed","fields":{"password":"wrong"}}',
                         '{"fields":{"password":123}}',
                         '{"FIELDS":{"password":"fixture-secret"},"PATH":"confirmed"}',
                         '{"path":"first","path":null,"fields":{"password":"fixture-secret"}}',
                         '{"fields":{"one":1},"fields":{"password":"fixture-secret"}}'):
            add(f"{action}-metadata-{len(result)}", [action, query], metadata=metadata)
        for metadata in ('{"fields":{"large":1e1000,"password":"fixture-secret"}}',
                         '{"fields":{"large":1e1000,"large":0,"password":"fixture-secret"}}',
                         '{"fields":{"obj":{"n":1e1000,"n":0},"password":"fixture-secret"}}',
                         '{"fields":{"obj":{"$serde_json::private::Number":"7"},"password":"fixture-secret"}}'):
            add(f"{action}-numeric-{len(result)}", [action, query], metadata=metadata)
        add(f"{action}-mutation-error", [action, query], action_exit=42)
        add(f"{action}-confirmation-error", [action, query], get_exit=1)
        for discovery in ("config", "managed", "path", "configured-missing", "missing", "directory", "non-executable", "malformed-config", "wrong-type-config"):
            add(f"{action}-{discovery}", [action, query], discovery=discovery,
                metadata='{"fields":{"password":"fixture-secret"}}',
                portable=discovery not in ("configured-missing", "missing", "directory", "non-executable", "malformed-config", "wrong-type-config"))
    for flag in ("--yes", "-y"):
        for code in (0, 1, 2, 42):
            add(f"delete-{flag}-{code}", ["delete", "work/item", flag], b"", get_exit=code)
    add("delete-mutation-error", ["delete", "work/item", "--yes"], b"", action_exit=42)
    add("delete-wrong-flag", ["delete", "work/item", "--force"], b"")
    for code in (0, 42):
        add(f"opaque-{code}", ["approval", "--output", "raw", "--json", "--", "-flag"],
            b"opaque stdin\n", passthrough=True, action_exit=code)
    for path in ("work/github.com/My Login", "work/<>&", "work/\u2028line\u2029", "work/./item"):
        for action in ("create", "set"):
            add(f"{action}-legal-path-{len(result)}", [action, path + (".password" if action == "set" else "")],
                metadata='{"fields":{"password":"fixture-secret"}}')
    for variable in ("SYMBRAIN_AUDIT_ENABLED", "SYMBRAIN_AUDIT_VERBOSE",
                     "SYMBRAIN_GATEWAY_IDENTITY_INJECTION", "SYMBRAIN_UPDATECHECK_ENABLED",
                     "SYMBRAIN_PATTERNS_ENABLED", "SYMBRAIN_MODULES_BROWSE",
                     "SYMBRAIN_MODULES_OPERATE", "SYMBRAIN_MODULES_SCOPE"):
        add(f"create-invalid-env-{variable}", ["create", "work/item"],
            extra_env={variable: "invalid"}, portable=False)
    for value in ("", "1", "t", "T", "true", "TRUE", "True", "0", "f", "F", "false", "FALSE", "False",
                  " true", "True ", "TrUe", "yes"):
        add(f"create-bool-env-{len(result)}", ["create", "work/item"],
            metadata='{"fields":{"password":"fixture-secret"}}',
            extra_env={"SYMBRAIN_AUDIT_ENABLED": value},
            portable=value in ("", "1", "t", "T", "true", "TRUE", "True", "0", "f", "F", "false", "FALSE", "False"))
    for value in ("", "0", "+1", "-1", "0001", "9223372036854775807", "-9223372036854775808",
                  "9223372036854775808", "-9223372036854775809", "1_000", " 1", "1 ", "+", "invalid"):
        add(f"create-int-env-{len(result)}", ["create", "work/item"],
            metadata='{"fields":{"password":"fixture-secret"}}',
            extra_env={"SYMBRAIN_PATTERNS_PROMOTION_THRESHOLD": value},
            portable=value in ("", "0", "+1", "-1", "0001", "9223372036854775807", "-9223372036854775808"))
    for config in ('default_profile = 0', 'default_profile = false', 'default_profile = 0.0',
                   'default_profile = []', 'audit = 1', 'audit = []',
                   '[audit]\nenabled = "true"', '[audit]\nenabled = ""',
                   '[audit]\nenabled = 0', '[audit]\nverbose = 0',
                   '[modules]\nbrowse = "True"', '[modules]\nbrowse = "invalid"',
                   '[patterns]\npromotion_threshold = "12"',
                   '[patterns]\npromotion_threshold = 1.5',
                   '[patterns]\npromotion_threshold = false',
                   '[patterns]\npromotion_threshold = "1_000"',
                   '[servers]\nvault = 1'):
        add(f"create-config-conversion-{len(result)}", ["create", "work/item"],
            metadata='{"fields":{"password":"fixture-secret"}}',
            config_text=config, portable=False)
    for project in ('[servers.vault]\nbinary_path = "<CHILD>"',
                    '[servers.vault]\nbinary_path = ""',
                    'broken =', '[audit]\nenabled = "invalid"',
                    'default_profile = 0'):
        add(f"create-project-config-{len(result)}", ["create", "work/item"],
            discovery="config", metadata='{"fields":{"password":"fixture-secret"}}',
            project_text=project, portable=False)
    return result


def run(binary, child, case, root):
    root.mkdir()
    home = root / "home"
    home.mkdir()
    env = {key: os.environ[key] for key in ("SystemRoot", "WINDIR", "COMSPEC", "PATHEXT") if key in os.environ}
    env.update(HOME=str(home), USERPROFILE=str(home), XDG_CONFIG_HOME=str(root / "config"),
               XDG_DATA_HOME=str(root / "data"), XDG_CACHE_HOME=str(root / "cache"),
               PATH=str(root / "empty-bin"), LANG="C.UTF-8", TZ="UTC",
               FAKE_VAULT_LOG=str(root / "calls"),
               FAKE_VAULT_ACTION_EXIT=str(case.get("action_exit", 0)),
               FAKE_VAULT_GET_EXIT=str(case.get("get_exit", 0)),
               FAKE_VAULT_METADATA=case.get("metadata") or "{}")
    if case.get("passthrough"):
        env["FAKE_VAULT_PASSTHROUGH"] = "1"
    env.update(case.get("extra_env", {}))
    discovery = case.get("discovery", "env")
    if discovery == "env":
        env["SYMBRAIN_SERVERS_VAULT_BINARY_PATH"] = str(child)
    elif discovery == "config":
        config = root / "config/symbrain/config.toml"
        config.parent.mkdir(parents=True)
        config.write_text("[servers.vault]\nbinary_path = " + json.dumps(str(child)) + "\n")
    elif discovery in ("managed", "path"):
        dest = (home / ".symaira/bin" if discovery == "managed" else root / "bin") / ("symvault.exe" if os.name == "nt" else "symvault")
        dest.parent.mkdir(parents=True)
        shutil.copyfile(child, dest)
        dest.chmod(0o755)
        if discovery == "path":
            env["PATH"] = str(dest.parent)
    elif discovery == "configured-missing":
        env["SYMBRAIN_SERVERS_VAULT_BINARY_PATH"] = str(root / "missing-symvault")
    elif discovery in ("directory", "non-executable"):
        destination = root / "unusable-symvault"
        if discovery == "directory":
            destination.mkdir()
        else:
            destination.write_text("plain data")
            destination.chmod(0o644)
        env["SYMBRAIN_SERVERS_VAULT_BINARY_PATH"] = str(destination)
    elif discovery in ("malformed-config", "wrong-type-config"):
        config = root / "config/symbrain/config.toml"
        config.parent.mkdir(parents=True)
        config.write_text("broken =" if discovery == "malformed-config" else "default_profile = 1\n")
        env["SYMBRAIN_SERVERS_VAULT_BINARY_PATH"] = str(child)
    if "config_text" in case:
        config = root / "config/symbrain/config.toml"
        config.parent.mkdir(parents=True, exist_ok=True)
        config.write_text(case["config_text"])
    if "project_text" in case:
        (root / ".symbrain.toml").write_text(case["project_text"].replace("<CHILD>", str(child).replace("\\", "\\\\")))
    process = subprocess.run([str(binary), "vault", *case["args"]], input=bytes.fromhex(case["input_hex"]),
                             env=env, cwd=root, capture_output=True, timeout=5)
    normalize = lambda data: data.decode(errors="backslashreplace").replace(str(root), "<ROOT>")
    return dict(exit_code=process.returncode, stdout=normalize(process.stdout),
                stderr=normalize(process.stderr), calls=(root / "calls").read_text() if (root / "calls").exists() else "")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--go-ref", required=True)
    parser.add_argument("--rust", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[2]
    ref = subprocess.check_output(["git", "rev-parse", args.go_ref + "^{commit}"], cwd=repo, text=True).strip()
    support = repo / "rust/symbrain-cli/tests/support/vault_child.rs"
    with tempfile.TemporaryDirectory(prefix="vault-oracle-") as directory:
        temporary = Path(directory)
        archive = temporary / "source.tar"
        with archive.open("wb") as output:
            subprocess.run(["git", "archive", ref], cwd=repo, stdout=output, check=True)
        source = temporary / "go-source"
        with tarfile.open(archive) as tree:
            tree.extractall(source, filter="data")
        go = temporary / ("symbrain-go.exe" if os.name == "nt" else "symbrain-go")
        env = dict(os.environ, GOTOOLCHAIN="go1.26.7", CGO_ENABLED="0")
        subprocess.run(["go", "build", "-mod=readonly", "-o", str(go), "./cmd/symbrain"], cwd=source, env=env, check=True)
        child = temporary / ("fake-vault.exe" if os.name == "nt" else "fake-vault")
        subprocess.run(["rustc", "--edition=2024", str(support), "-o", str(child)], check=True)
        results = []
        for index, case in enumerate(cases()):
            expected = run(go, child, case, temporary / f"go-{index}")
            actual = run(args.rust.resolve(), child, case, temporary / f"rust-{index}") if args.rust else None
            if actual is not None and actual != expected:
                print("MISMATCH", case["name"], json.dumps(dict(expected=expected, actual=actual)))
            results.append(dict(case=case, go=expected, rust=actual, matched=actual == expected if actual is not None else None))
        report = dict(go_oracle_ref=ref, platform=os.name, total=len(results),
                      source_sha256={str(path): hashlib.sha256((source/path).read_bytes()).hexdigest() for path in
                                     ("cmd/symbrain/main.go", "cmd/symbrain/cmd_passthrough.go", "cmd/symbrain/cmd_vault_create.go", "cmd/symbrain/cmd_vault_editdel.go", "internal/config/config.go", "internal/broker/client.go", "go.mod", "go.sum")},
                      child_sha256=hashlib.sha256(support.read_bytes()).hexdigest(), results=results)
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(json.dumps(report, indent=2) + "\n")
        if args.rust:
            matched = sum(item["matched"] for item in results)
            print(f"{matched}/{len(results)} actual process comparisons match")
            if matched != len(results):
                raise SystemExit(1)


if __name__ == "__main__":
    main()
