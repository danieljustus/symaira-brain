"""Actual native provider paths: public resolver API versus its owned CLI route."""
from __future__ import annotations
import base64
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import daemon_state_key as key


def encode(value: bytes) -> str:
    return base64.b64encode(value).decode()


def streams(result) -> dict:
    return {"exit": result.returncode, "stdout_base64": encode(result.stdout),
            "stderr_base64": encode(result.stderr)}


def queries(ledger: Path) -> list:
    return [json.loads(line) for line in ledger.read_text().splitlines()] if ledger.exists() else []


def observe(probe: Path, binary: Path, fixture: Path, go: str) -> dict:
    with tempfile.TemporaryDirectory(prefix="bk-owner-path-", dir=key.registry.process.private_temporary_parent()) as directory:
        root = Path(directory); env = key.registry.environment(root)
        env.update(SYMBROWSE_KEY_PROBE_MODE="ab", SYMBROWSE_KEYCHAIN_PROBE_MODE="44",
                   SYMBROWSE_ENCRYPTION_KEY=key.KEY)
        source = root / "path-control.go"
        template = key.HERE / "startup_key_path_control.go.in"
        shutil.copyfile(template, source)
        control = root / ("path-control.exe" if os.name == "nt" else "path-control")
        subprocess.run([go, "build", "-o", str(control), str(source)],
                       env=dict(os.environ, CGO_ENABLED="0", GOTOOLCHAIN="local", GO111MODULE="off"),
                       capture_output=True, check=True, timeout=120)
        names = ["ascii", "unicode-ä"]
        if os.name == "posix": names.append(os.fsdecode(b"raw-\xff"))
        expected = {"configured": True, "key_source": "symvault", "error": ""}
        rows = []
        for name in names:
            folder = root / name; folder.mkdir(mode=0o700)
            provider = folder / ("symvault.exe" if os.name == "nt" else "symvault")
            shutil.copyfile(fixture, provider); provider.chmod(0o700)
            ledger = root / "queries.jsonl"; ledger.unlink(missing_ok=True)
            actual_env = dict(env, PATH=str(folder), SYMBROWSE_KEY_PROBE_LEDGER=str(ledger))
            public = subprocess.run([str(probe), str(provider), str(binary)], env=actual_env,
                                    cwd=root, capture_output=True, timeout=5)
            public_queries = queries(ledger)
            assert public.returncode == 0 and not public.stderr
            assert json.loads(public.stdout) == expected and len(public_queries) == 1
            ledger.unlink()
            # Keep the private lifetime writer open until this real supervisor
            # exits normally. No key material travels in its public argv.
            with subprocess.Popen([str(binary), "--internal-startup-key-provider", str(provider),
                                   "1000", "get", "symbrowse/encryption-key"], cwd=root, env=actual_env,
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                  start_new_session=(os.name == "posix")) as child:
                try:
                    child.wait(timeout=5)
                    stdout, stderr = child.communicate(timeout=2)
                finally:
                    if child.poll() is None: child.kill(); child.wait(timeout=2)
            internal = {"exit": child.returncode, "stdout_base64": encode(stdout), "stderr_base64": encode(stderr)}
            internal_queries = queries(ledger)
            assert child.returncode == 0 and json.loads(stdout) == {"value": key.KEY}
            assert json.loads(stderr) == {"result": "Exited", "status": 0} and len(internal_queries) == 1
            for query in public_queries + internal_queries:
                assert query["name"] == "symvault" and query["arguments"] == ["get", "symbrowse/encryption-key"]
            rows.append({"name_filesystem_base64": encode(os.fsencode(name)),
                         "provider_path_filesystem_base64": encode(os.fsencode(provider)),
                         "public": streams(public), "public_queries": public_queries,
                         "internal": internal, "internal_queries": internal_queries, "matches": True})
            if name == "unicode-ä":
                # The same executable control runs on Windows and Unix. The
                # unchanged Unicode directory exists; its replacement does not.
                ledger.unlink()
                mutated = subprocess.run([str(control), str(provider), str(binary)], cwd=root,
                    env=dict(actual_env, SYMBROWSE_STARTUP_PATH_PROBE=str(probe)),
                    capture_output=True, timeout=5)
                mutated_queries = queries(ledger)
                assert mutated.returncode == 0 and not mutated.stderr
                rejected = json.loads(mutated.stdout) != expected and not mutated_queries
                assert rejected, "path corruption control was falsely accepted"
                mutation = {"streams": streams(mutated), "queries": mutated_queries, "rejected": rejected}
        return {"cases": rows, "negative_control": mutation,
                "probe_binary_sha256": key.registry.process.digest(probe),
                "control_binary_sha256": key.registry.process.digest(control),
                "control_source_sha256": key.registry.process.digest(template),
                "scope": "native ASCII/Unicode paths; additional byte-FF path only on Unix; real public API and internal route, no GoCLI regression claim"}
