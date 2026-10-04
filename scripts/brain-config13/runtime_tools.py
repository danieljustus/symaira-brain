"""Owned runtime preparation/bindings; no corpus or acceptance projection."""
import argparse
import ast
import base64
import gzip
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess


def sha(data):
    return hashlib.sha256(data).hexdigest()


def save(path, record):
    path.write_text(json.dumps(record, indent=2) + "\n")


def source(root, out):
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root).decode().strip()
    assert not subprocess.check_output(["git", "status", "--porcelain"], cwd=root)
    checkpoint = json.loads((root / "migration/evidence/brain-config13/corrections-source-only/checkpoint.json").read_text())
    for name, expected in checkpoint["candidate_source_sha256"].items():
        assert sha((root / name).read_bytes()) == expected, name
    for name, expected in checkpoint["pinned_source_sha256"].items():
        assert sha(Path(name).read_bytes()) == expected, name
    tracked = subprocess.check_output(["git", "ls-files", "-z"], cwd=root).decode().split("\0")
    files = [name for name in tracked if name and (name.startswith(("rust/", "scripts/"))
             or name in ("Cargo.toml", "Cargo.lock"))]
    save(out / "source.json", dict(head=head, source={name: sha((root/name).read_bytes()) for name in files},
         validated_checkpoint_entries=len(checkpoint["candidate_source_sha256"]),
         pinned_references=len(checkpoint["pinned_source_sha256"]), clean=True))


def binaries(root, out, target):
    records = {}
    names = {"cli": target / "debug/symbrain", "guard": target / "debug/symguard"}
    for name in ("brain_config13_probe", "brain_config13_fault", "brain_config13_correction_fault", "brain_config13_owned_vault"):
        names[name] = target / "debug/examples" / name
    for role, original in names.items():
        copied = out / "bin" / role
        copied.parent.mkdir(exist_ok=True)
        shutil.copyfile(original, copied)
        copied.chmod(original.stat().st_mode & 0o777)
        assert original.read_bytes() == copied.read_bytes()
        records[role] = dict(original=str(original), copy=str(copied), sha256=sha(copied.read_bytes()))
    artifacts = []
    for line in (out / "build.stdout").read_text().splitlines():
        try:
            item = json.loads(line)
        except ValueError:
            continue
        if item.get("reason") == "compiler-artifact":
            artifacts.append(item)
    assert any(item.get("executable") == str(target / "debug/symbrain") for item in artifacts)
    save(out / "binaries.json", dict(source_head=json.loads((out/"source.json").read_text())["head"],
         actual=records, cargo_artifacts=artifacts, no_native_windows_claim=True))


def archive(root, out, target):
    users = []
    for proc in Path("/proc").glob("[0-9]*"):
        for link in [proc / "exe", proc / "cwd", *list((proc / "fd").glob("*"))]:
            try:
                if Path(os.readlink(link)).is_relative_to(target):
                    users.append(str(link))
            except OSError:
                pass
    assert not users, users
    dest = out / "failed-actual-binaries"
    dest.mkdir(exist_ok=False)
    records = []
    for path in sorted(target.rglob("*")):
        if not path.is_file() or path.is_symlink():
            continue
        with path.open("rb") as stream:
            magic = stream.read(4)
        if magic != b"\x7fELF" and path.suffix != ".rlib":
            continue
        data = path.read_bytes()
        digest = sha(data)
        payload = dest / (digest + ".gz")
        if not payload.exists():
            assert shutil.disk_usage(root).free >= 700*1024*1024, "cannot archive below allocated floor"
            payload.write_bytes(gzip.compress(data, mtime=0))
        assert gzip.decompress(payload.read_bytes()) == data
        records.append(dict(path=str(path), bytes=len(data), sha256=digest, gzip=str(payload),
                            gzip_sha256=sha(payload.read_bytes())))
    save(dest / "receipt.json", dict(source=json.loads((out/"source.json").read_text()), users=users,
         all_actual_ELF_and_rlib=records, all_roundtrips=True, nothing_deleted=True))


def historical(root, out):
    scripts = [Path("/tmp") / ("symaira-brain-config13-" + name + ".py")
               for name in ("inventory", "boundaries", "patterns", "admission")]
    scripts += sorted(Path("/tmp").glob("symaira-doctor765-sixth-independent-supplement-*.py"))
    dest = out / "historical"
    dest.mkdir()
    source_head = json.loads((out / "source.json").read_text())["head"]
    mapping = {
        "/workspace/oracles/symbrain-go-dcddcef0": str(out / "go-cli"),
        "/tmp/symaira-doctor765-sixth-58fe-cli": str(out / "bin/cli"),
        "/workspace/symaira-doctor765-stdout": str(root),
        "58fe50b9d275f0cc8a7ab6a8252a6595a58e52ab": source_head,
        "256e5efe3d05b9fe5c8afa41d9821a13ee2dee32de9719a4acd7e79364fde8ee": sha((out/"bin/cli").read_bytes()),
    }
    records = []
    class Rebind(ast.NodeTransformer):
        def visit_Constant(self, node):
            if isinstance(node.value, str):
                value = mapping.get(node.value, node.value)
                if value.startswith(("/tmp/symaira-brain-config13-", "/tmp/symaira-doctor765-sixth-independent-supplement-")):
                    value = str(dest / Path(value).name)
                if value != node.value:
                    return ast.copy_location(ast.Constant(value=value), node)
            return node
    for script in scripts:
        original = script.read_bytes()
        (dest / (script.name + ".original.gz")).write_bytes(gzip.compress(original, mtime=0))
        tree = Rebind().visit(ast.parse(original))
        prepared = ast.unparse(ast.fix_missing_locations(tree)) + "\n"
        new = dest / script.name
        new.write_text(prepared)
        records.append(dict(original=str(script), original_sha256=sha(original),
                            rebound=str(new), rebound_sha256=sha(new.read_bytes())))
    save(dest / "bindings.json", dict(source_head=source_head, binding_only=mapping, records=records,
         historical_census_not_native_acceptance=True, original_criteria_unchanged=True))


def control(out, report, mode, family):
    record = json.loads(report.read_text())
    assert record["source_head"] == json.loads((out/"source.json").read_text())["head"]
    assert record["total"] == 1 and record["equal"] == 0
    delegated = out/"bin"/("brain_config13_probe" if family=="values" else "cli")
    assert record["delegated_native_sha256"] == sha(delegated.read_bytes())
    row = record["observations"][0]
    go, native = row["go"], row["native"]
    stdout = lambda value: base64.b64decode(value["stdout_b64"])
    stderr = lambda value: base64.b64decode(value["stderr_b64"])
    if family=="correction":
        assert record["status"] == "finished" and go["exit"] == 0
        if mode=="wrong-owner":
            assert native["exit"]==0 and stdout(go)==b"owned-vault:lexical-vault\n"
            assert stdout(native)==b"owned-vault:physical-vault\n"
        elif mode=="repair-codex":
            assert native["exit"]==0 and b"owned\xe2\x82" in stdout(go) and b"owned\xe2\x82" not in stdout(native)
        else:
            assert native["exit"]==2 and stderr(native)==b"injected rejection of Go-admitted initial marker\n"
            assert native["before"]==native["after"]
    elif mode=="wrong-value":
        assert go["exit"]==native["exit"]==0
        expected=json.loads(stdout(go));expected["audit.enabled"]=not expected["audit.enabled"]
        assert json.loads(stdout(native))==expected
    else:
        assert go["exit"]==2 and go["before"]==go["after"]
        if mode=="wrong-exit":
            assert native["exit"]==1
        elif mode=="wrong-field-error":
            assert native["exit"]==2 and stderr(native).endswith(b"injected wrong field\n")
        elif mode=="premature-http":
            assert native["exit"]==2 and "/config13-injected-admission" in native["owned_release_requests"]
        else:
            injected=("home/config13-injected-admission").encode().hex()
            assert native["exit"]==2 and injected not in native["before"] and injected in native["after"]
    save(report.with_suffix(".verified.json"), dict(actual_report_sha256=sha(report.read_bytes()),
         mode=mode, family=family, intended_fault_verified=True, incidental_failure_rejected=True))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("source", "binaries", "archive", "historical", "control"))
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--target", type=Path)
    parser.add_argument("--report", type=Path)
    parser.add_argument("--control-mode")
    parser.add_argument("--family")
    args = parser.parse_args()
    functions = {"source": source, "historical": historical, "binaries": binaries, "archive": archive}
    if args.mode == "control":
        control(args.out.resolve(), args.report, args.control_mode, args.family)
    elif args.mode in ("binaries", "archive"):
        functions[args.mode](args.root.resolve(), args.out.resolve(), args.target.resolve())
    else:
        functions[args.mode](args.root.resolve(), args.out.resolve())


if __name__ == "__main__":
    main()
