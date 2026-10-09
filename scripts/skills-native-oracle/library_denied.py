"""Prepared real permission-denied library pairs, retaining observer authority."""
import hashlib
import os
from pathlib import Path
import shutil
import stat

from compare import matched
from fixtures import fixture_command, remove_tree, write


def required_ids():
    return ["library-denied-cli-table", "library-denied-cli-json", "library-denied-mcp-line", "library-denied-mcp-framed"]


def acl_tool():
    return Path(os.environ["SystemRoot"]) / "System32/icacls.exe"


def run_pairs(report, save, binaries, root, retained, env, invoke, incoming):
    requests = [(required_ids()[0], ["skills", "list"], None),
                (required_ids()[1], ["skills", "list", "--json"], None),
                (required_ids()[2], ["mcp", "--profile-file", str(root / "profile.toml")], incoming("skills_list", {})),
                (required_ids()[3], ["mcp", "--profile-file", str(root / "profile.toml")], incoming("skills_list", {}, True))]
    for identity, argv, data in requests:
        pair = {"id": identity, "mcp": data is not None, "stdin_hex": data.hex() if data else None, "matched": False}
        report["results"].append(pair)
        save()
        for flavor in ("go", "rust"):
            remove_tree(root)
            shutil.copytree(retained, root, symlinks=True)
            document = root / "data/symbrain/skills/library/denied/SKILL.md"
            write(document, "---\nname: denied\ndescription: owned\n---\nbody\n")
            assert document.is_relative_to(root) and not document.is_symlink()
            original = document.read_bytes()
            proof = {"path": str(document), "sha256_before": hashlib.sha256(original).hexdigest(), "commands": []}
            pair[flavor + "_permission_fixture"] = proof
            # Harness-only observer is opened before denial and never inherited
            # by either product. Its inode is checked at the later snapshot.
            with document.open("rb") as observer:
                before = os.fstat(observer.fileno())
                assert stat.S_ISREG(before.st_mode)
                if os.name == "nt":
                    tool = acl_tool()
                    proof["acl_tool"] = {"path": str(tool), "sha256": hashlib.sha256(tool.read_bytes()).hexdigest()}
                    out = fixture_command([str(tool), str(document), "/deny", "*S-1-1-0:(RD)"], root, env, proof["commands"])
                    assert out.returncode == 0, "owned read-data denial must succeed"
                    initial = fixture_command([str(tool), str(document)], root, env, proof["commands"])
                    assert initial.returncode == 0
                    proof["acl_before_hex"] = initial.stdout.hex()
                else:
                    document.chmod(0)
                    proof["mode_before"] = stat.S_IMODE(document.stat().st_mode)
                    assert proof["mode_before"] == 0
                # A privileged Unix owner must fail fixture admission instead of
                # calling a readable document an actual permission-denial proof.
                try:
                    with document.open("rb") as attempt:
                        attempt.read(1)
                except PermissionError:
                    proof["actual_read_denied"] = True
                else:
                    raise AssertionError("owned fixture is readable: no genuine permission-denied observation")
                save()
                pair[flavor] = {}
                invoke(binaries[flavor], argv, env, root, data,
                       retained_reads={document: observer}, record=pair[flavor])
                observer.seek(0)
                proof["sha256_after"] = hashlib.sha256(observer.read()).hexdigest()
                assert proof["sha256_after"] == proof["sha256_before"]
                if os.name == "nt":
                    after = fixture_command([str(tool), str(document)], root, env, proof["commands"])
                    assert after.returncode == 0
                    proof["acl_after_hex"] = after.stdout.hex()
                    assert proof["acl_after_hex"] == proof["acl_before_hex"], "product changed owned ACL"
                    assert hashlib.sha256(tool.read_bytes()).hexdigest() == proof["acl_tool"]["sha256"]
                else:
                    proof["mode_after"] = stat.S_IMODE(document.stat().st_mode)
                    assert proof["mode_after"] == proof["mode_before"], "product changed owned permissions"
                save()
        try:
            pair["matched"] = matched(pair["go"], pair["rust"], root, data is not None)
        except Exception as error:
            pair["comparison_error"] = {"type": type(error).__name__, "message": str(error)}
            raise
        save()
