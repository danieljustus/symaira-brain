"""Source/retention checks only: no product, SDK, compiler, kernel or port run."""
import ast
import hashlib
import json
from pathlib import Path
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[4]
BASE = "8e41529c86c0415c8ab71ce403a588079320ea61"
OUT = Path(__file__).resolve().parent


def original(name):
    return subprocess.check_output(["git", "show", BASE + ":" + name], cwd=ROOT)


def function(text, name):
    return next(node for node in ast.parse(text).body if isinstance(node, ast.FunctionDef) and node.name == name)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    harness = ROOT / "browse/port/harness"
    parsed = sorted(path for path in harness.glob("*.py"))
    for path in parsed:
        ast.parse(path.read_bytes(), filename=str(path))
    unchanged = {}
    for file, names in {
        "daemon_process.py": ["observe", "normalized", "compare", "controls", "private_temporary_parent", "absent_keychain_path"],
        "daemon_registry.py": ["observe", "environment", "start", "request", "stop", "oracle_api"],
        "daemon_registry_cli.py": ["observe", "compare"],
    }.items():
        old, new = original("browse/port/harness/" + file), (harness / file).read_bytes()
        for name in names:
            assert ast.dump(function(old, name)) == ast.dump(function(new, name)), (file, name)
        unchanged[file] = names
    compare = "browse/port/harness/registry_compare.py"
    assert original(compare) == (ROOT / compare).read_bytes()
    old_transport = original("browse/crates/symbrowse-daemon/src/client/transport.rs").decode()
    new_transport = (ROOT / "browse/crates/symbrowse-daemon/src/client/transport.rs").read_text()
    for start, end in [("    #[cfg(unix)]", "    #[cfg(windows)]"),
                       ("            tokio::time::timeout(timeout, async {", "    #[cfg(not(any(unix, windows)))]")]:
        assert old_transport.split(start, 1)[1].split(end, 1)[0] == new_transport.split(start, 1)[1].split(end, 1)[0]
    assert "spawn_blocking" not in new_transport and "interprocess" not in new_transport.split("async fn connect_windows", 1)[0]
    assert "let path = self.options.socket_path.to_string_lossy();" in old_transport
    assert "let path = self.options.socket_path.to_string_lossy();" in new_transport
    assert "connect_windows(path.as_ref())" in new_transport
    assert "ClientOptions::new().open(path)" in new_transport
    windows_tests = (ROOT / "browse/crates/symbrowse-daemon/tests/windows_client_deadlines.rs").read_text()
    old_tests = subprocess.check_output(["git", "show", "591248a89f56c7b810daf9fa1a036ec20061aaec:browse/crates/symbrowse-daemon/tests/windows_client_deadlines.rs"], cwd=ROOT, text=True)
    assert old_tests.split("fn blocked_peer(", 1)[1] == windows_tests.split("fn blocked_peer(", 1)[1]
    scenarios = ["unread-request", "full-request", "busy-instance", "eight-clients",
                 "endpoint-ascii", "endpoint-unicode", "endpoint-lossy"]
    assert all('"' + name + '"' in windows_tests for name in scenarios)
    relation = json.loads((OUT.parent / "endpoint-relation-finding/receipt.json").read_text())
    relation_archive = OUT.parent / "endpoint-relation-finding/pre-correction-source.tar.gz"
    assert digest(relation_archive.read_bytes()) == relation["archive_sha256"]
    with tarfile.open(relation_archive, "r:gz") as saved:
        assert len(saved.getmembers()) == len(relation["members"]) == 11
        for row in relation["members"]:
            data = saved.extractfile(row["member"]).read()
            assert len(data) == row["bytes"] and digest(data) == row["sha256"]
            if "git_head" in row:
                assert subprocess.check_output(["git", "show", row["git_head"] + ":" + row["path"]], cwd=ROOT) == data
            else:
                assert Path(row["local_reference"]).read_bytes() == data
    for file in ["client.rs", "client/errors.rs", "client/process.rs", "server/windows.rs", "server/connection.rs", "protocol.rs"]:
        name = "browse/crates/symbrowse-daemon/src/" + file
        assert original(name) == (ROOT / name).read_bytes()
    capture = (harness / "registry_cli_process.py").read_text()
    assert "CLI_TIMEOUT = 15" in capture and "communicate(" not in capture and "subprocess.PIPE" not in capture
    assert capture.index("receipt.write_text(") < capture.index("    if failure is not None:")
    progress = (harness / "registry_progress.py").read_text()
    assert "MAX_EVENTS = 8192" in progress and "MAX_EVENT_BYTES = 8192" in progress
    workflow = (ROOT / ".github/workflows/browse-daemon-native.yml").read_text()
    assert "timeout-minutes: 30" in workflow and "native_windows_client_deadlines -- --ignored --exact --nocapture" in workflow
    retention = json.loads((OUT.parent / "original/retention.json").read_text())
    archive = OUT.parent / "original/raw-and-source.tar.gz"
    assert digest(archive.read_bytes()) == retention["archive_sha256"]
    with tarfile.open(archive, "r:gz") as saved:
        assert len(saved.getmembers()) == len(retention["members"]) == 324
        for row in retention["members"]:
            data = saved.extractfile(row["member"]).read()
            assert len(data) == row["bytes"] and digest(data) == row["sha256"]
            prefix = str(ROOT) + "/"
            if row["original"].startswith(prefix):
                assert original(row["original"].removeprefix(prefix)) == data
            else:
                assert digest(Path(row["original"]).read_bytes()) == row["sha256"]
    scope = ["browse/crates", "browse/port/harness", ".github/workflows/browse-daemon-native.yml",
             "docs/adr/native-client-deadlines-801.md", "migration/evidence/daemon-client-deadlines-801"]
    # Current main is an explicitly inherited parent. Its excluded sparse
    # Brain/Skills files are not authored Browse changes or absent source bytes.
    changed = subprocess.check_output(["git", "diff", "--name-only", BASE, "--", *scope], cwd=ROOT, text=True).splitlines()
    untracked = subprocess.check_output(["git", "ls-files", "--others", "--exclude-standard"], cwd=ROOT, text=True).splitlines()
    names = sorted(set(changed + untracked))
    assert not any(name.endswith(".go") for name in changed)
    for name in ("browse/Cargo.lock", "browse/Cargo.toml"):
        assert original(name) == (ROOT / name).read_bytes()
    counts = {name: len((ROOT / name).read_text().splitlines()) for name in names if name.endswith(".rs")}
    assert max(counts.values()) < 400
    maps = [{"path": name, "sha256": digest((ROOT / name).read_bytes()), "bytes": (ROOT / name).stat().st_size}
            for name in names if (ROOT / name).is_file() and not name.startswith(str(OUT.relative_to(ROOT)))]
    result = {"kind": "SOURCE_ONLY; synthetic accounting unit tests separate; no kernel/product/SDK/compiler/native proof",
              "Python_ASTs": len(parsed), "original_functions_AST_unchanged": unchanged,
              "registry_comparator_byte_identical": True, "Unix_transport_and_Windows_IO_body_byte_identical": True,
              "frame_errors_server_and_child_deadlines_byte_identical": True, "original_archive_roundtrips": 324,
              "Rust_line_counts": counts, "prepared_Windows_scenarios": scenarios,
              "prior_shared_lossy_endpoint_relation_retained": True, "endpoint_finding_roundtrips": 11, "source_maps": maps, "explicit_Browse_correction_scope": scope,
              "candidate_runtime_executions": 0}
    (OUT / "checks.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({k: v for k, v in result.items() if k != "source_maps"}))


if __name__ == "__main__":
    main()
