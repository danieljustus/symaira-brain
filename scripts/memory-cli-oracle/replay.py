#!/usr/bin/env python3
"""Execute scoped native memory CLI contracts against an immutable Go process."""
import argparse
import base64
from contextlib import closing
import hashlib
import http.server
import io
import json
import os
from pathlib import Path
import re
import sqlite3
import subprocess
import tarfile
import tempfile
import threading

import cases
import corrupt_reads
import path_cases

ORACLE = "dcddcef0df5789123c7c9a7ebe6e01f10e941f2c"
CONFIGKIT_SHA = "bd812ed747352c76b9f223a19ec64970b61fda59050f7ed436f5b2e74b997218"


def run(args, cwd, env=None):
    return subprocess.run(args, cwd=cwd, env=env, check=True, capture_output=True)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def isolated_env(root):
    env = {key: value for key, value in os.environ.items()
           if not key.startswith(("SYMMEMORY_", "SYMBRAIN_MEMORY_"))}
    env.update(HOME=str(root), USERPROFILE=str(root), XDG_CONFIG_HOME=str(root / "config"),
               XDG_DATA_HOME=str(root / "data"), XDG_CACHE_HOME=str(root / "cache"),
               XDG_STATE_HOME=str(root / "state"), PATH="",
               SYMBRAIN_GO_BINARY=str(root / "absent-go"))
    return env


def output(binary, args, root, env):
    result = subprocess.run([str(binary), "memory", *args], cwd=root, env=env,
                            capture_output=True, timeout=15)
    return dict(exit=result.returncode, stdout=base64.b64encode(result.stdout).decode(),
                stderr=base64.b64encode(result.stderr).decode())


def arguments(go, rust, family):
    records = []
    for index, args in enumerate(cases.arguments()):
        with tempfile.TemporaryDirectory(prefix="memory-cli-args-") as temporary:
            root = Path(temporary)
            env = isolated_env(root)
            first, second = (output(binary, args, root, env) for binary in (go, rust))
            records.append(dict(family="arguments", index=index, args_base64=[
                base64.b64encode(value if isinstance(value, bytes) else value.encode()).decode()
                for value in args], match=first == second, go=first, rust=second))
    family.extend(records)


def seed(go, root, env):
    for label, path in (("default", root / "data/symbrain/memory/default.db"),
                        ("configured", root / "configured.db"),
                        ("project", root / "project.db"), ("env", root / "env.db")):
        result = output(go, ["list", "--db", str(path)], root, env)
        assert result["exit"] == 0, result
        with closing(sqlite3.connect(path)) as database, database:
            database.execute(
                "INSERT INTO memories(id,content,scope,kind,created_at,updated_at,metadata,embedding) "
                "VALUES(?,?,'global','reference','2000-01-01 00:00:00 +0000 UTC',"
                "'2000-01-01 00:00:00 +0000 UTC','{}',X'')", (label, label))


def snapshot(root):
    # Semantic database state is strict; journal/header bytes and file mtimes
    # are not a data contract. Each column/blob and every row is retained.
    state = {}
    for path in sorted(root.rglob("*.db")):
        tables = {}
        with closing(sqlite3.connect(path)) as database, database:
            names = database.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            for (name,) in names.fetchall():
                sql_name = '"' + name.replace('"', '""') + '"'
                rows = database.execute("SELECT * FROM " + sql_name).fetchall()
                serialized = [[{"blob_base64": base64.b64encode(value).decode()}
                               if isinstance(value, bytes) else value for value in row] for row in rows]
                tables[name] = sorted(serialized, key=lambda row: json.dumps(row, sort_keys=True))
        state[str(path.relative_to(root))] = tables
    return state


def configuration(go, rust, repo, records, expected_fields):
    schema = repo / "rust/symbrain-cli/src/memory_cli/config_schema.rs"
    fields = re.findall(r'\("([a-z_0-9.]+)", Kind::(\w+)\)', schema.read_text())
    assert len(fields) == 86, "typed schema coverage changed; reconcile immutable Go fields"
    assert fields == expected_fields, "native typed schema differs from reflected immutable Go"
    for index, (name, glob, project, overrides) in enumerate(cases.configuration(fields)):
        with tempfile.TemporaryDirectory(prefix="memory-cli-config-") as temporary:
            root = Path(temporary)
            env = isolated_env(root)
            seed(go, root, env)
            config = root / "config/symmemory/config.toml"
            config.parent.mkdir(parents=True)
            config.write_bytes(glob.encode())
            (root / ".symmemory.toml").write_bytes(project.encode())
            env.update(overrides)
            before = snapshot(root)
            first = output(go, ["list"], root, env)
            after_go = snapshot(root)
            second = output(rust, ["list"], root, env)
            after_rust = snapshot(root)
            unchanged = before == after_go == after_rust
            records.append(dict(family="configuration", index=index, name=name, global_toml=glob,
                                project_toml=project, overrides=overrides,
                                match=first == second and first["exit"] == 0 and unchanged,
                                database_state_unchanged=unchanged,
                                database_state_sha256=digest(json.dumps(before, sort_keys=True).encode()),
                                go=first, rust=second))


def seeded_reads(go, rust, records):
    with tempfile.TemporaryDirectory(prefix="memory-cli-read-") as temporary:
        root = Path(temporary)
        env = isolated_env(root)
        path = root / "memory.db"
        assert output(go, ["list", "--db", str(path)], root, env)["exit"] == 0
        with closing(sqlite3.connect(path)) as database, database:
            database.executemany(
                "INSERT INTO memories(id,content,scope,kind,created_at,updated_at,metadata,embedding) "
                "VALUES(?,?,?,?, '2000-01-01 00:00:00 +0000 UTC',"
                "'2000-01-01 00:00:00 +0000 UTC','{}',X'')",
                [(f"m-{index:04}", f"Unicode ä\tline {index}\n&<> β", "global" if index % 2 else "project",
                  "project" if index % 3 else "reference") for index in range(1100)])
            database.executemany(
                "INSERT INTO query_log(id,actor,scope,tool,query_text,duration_ms,created_at) "
                "VALUES(?,?,'global',?,?,3,'2000-01-01 00:00:00 +0000 UTC')",
                [(f"q-{index:03}", "claude" if index % 2 else "codex", "memory_list",
                  f"Unicode β query {index}") for index in range(80)])
            database.executemany(
                "INSERT INTO rules(id,content,scope,metadata,created_at,updated_at,created_by,updated_by) "
                "VALUES(?,?,?,?,'2000-01-01 00:00:00 +0000 UTC',"
                "'2000-01-01 00:00:00 +0000 UTC',?,?)",
                [(f"r-{index}", f"Unicode rule β &<>\u2028\u2029 {index}", scope,
                  json.dumps({"a<&>": "value &<>\u2028\u2029 β"}),
                  "actor &<>\u2028\u2029", "updater &<>\u2028\u2029")
                 for index, scope in enumerate(("global", "project", "agent"))])
        shapes = [
            ["list"], ["list", "--limit", "0"], ["list", "--limit=-1"],
            ["list", "--limit=0x10"], ["list", "-l", "075"], ["list", "--limit=1001"],
            ["list", "--scope=global", "-s=project"], ["list", "-s=project", "--scope=global"],
            ["rules"], ["rules", "--scope=global", "-s=project"],
            ["rules", "-s=project", "--scope=global"],
            ["query-log"], ["query-log", "--limit=0"], ["query-log", "--limit=0x10"],
            ["query-log", "--actor=claude"], ["query-log", "--actor=claude", "--actor=codex"],
        ]
        shapes += [[*shape, "--json"] for shape in shapes]
        before = snapshot(root)
        for index, shape in enumerate(shapes):
            args = [*shape, "--db", str(path)]
            first, second = (output(binary, args, root, env) for binary in (go, rust))
            unchanged = before == snapshot(root)
            populated = shape[0] != "rules" or (first["stdout"] and second["stdout"]
                and b"r-" in base64.b64decode(first["stdout"])
                and b"r-" in base64.b64decode(second["stdout"]))
            records.append(dict(family="seeded_reads", index=index, args=shape,
                                match=first == second and first["exit"] == 0 and unchanged and bool(populated),
                                database_state_unchanged=unchanged,
                                populated_rule_output=bool(populated) if shape[0] == "rules" else None,
                                go=first, rust=second))


def configured_search(go, rust, records):
    requests = []

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_POST(self):
            body = self.rfile.read(int(self.headers["Content-Length"]))
            requests.append(dict(path=self.path, body=json.loads(body)))
            data = json.dumps({"data": [{"index": 0, "embedding": [0.25] * 768}]}).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def log_message(self, *_):
            pass

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        for index, suffix in enumerate(("/api/embeddings", "/v1/embeddings", "/")):
            with tempfile.TemporaryDirectory(prefix="memory-cli-search-config-") as temporary:
                root = Path(temporary)
                env = isolated_env(root)
                path = root / "memory.db"
                assert output(go, ["list", "--db", str(path)], root, env)["exit"] == 0
                config = root / "config/symmemory/config.toml"
                config.parent.mkdir(parents=True)
                config.write_bytes((f'ollama.url="http://127.0.0.1:{server.server_port}{suffix}"\n'
                                    'ollama.model="global-model"\n').encode())
                model = "global-model"
                if index == 1:
                    (root / ".symmemory.toml").write_bytes(b'ollama.model="project-model"\n')
                    model = "project-model"
                if index == 2:
                    env["SYMMEMORY_OLLAMA_MODEL"] = "env-model"
                    model = "env-model"
                before = snapshot(root)
                requests.clear()
                args = ["search", "Unicode β query", "--db", str(path), "--json"]
                first = output(go, args, root, env)
                second = output(rust, args, root, env)
                expected_request = dict(path="/v1/embeddings", body={"model": model, "input": ["Unicode β query"]})
                transport = requests == [expected_request, expected_request]
                unchanged = before == snapshot(root)
                records.append(dict(family="configured_search", index=index, suffix=suffix,
                                    match=first == second and first["exit"] == 0 and transport and unchanged,
                                    requests=list(requests), transport_match=transport,
                                    database_state_unchanged=unchanged, go=first, rust=second))
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--go", type=Path, help="optional prebuilt immutable oracle; SHA recorded")
    parser.add_argument("--rust", type=Path)
    parser.add_argument("--family", choices=("all", "arguments", "configuration", "seeded_reads", "configured_search", "database_paths", "corrupt_reads"), default="all")
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[2]
    rust = (args.rust or repo / "target/debug" / ("symbrain.exe" if os.name == "nt" else "symbrain")).resolve()
    archive = run(["git", "archive", ORACLE], repo).stdout
    go_cache = run(["go", "env", "GOMODCACHE", "GOCACHE"], repo).stdout.decode().splitlines()
    with tempfile.TemporaryDirectory(prefix="memory-cli-oracle-") as temporary:
        root = Path(temporary)
        source = root / "source"
        source.mkdir()
        with tarfile.open(fileobj=io.BytesIO(archive)) as files:
            files.extractall(source, filter="data")
        home = root / "build-home"
        home.mkdir()
        env = isolated_env(home)
        env.update(PATH=os.environ.get("PATH", ""), GOWORK="off", GOENV="off",
                   GOMODCACHE=go_cache[0], GOCACHE=go_cache[1], CGO_ENABLED="0")
        run(["go", "mod", "download", "github.com/danieljustus/symaira-corekit@v0.17.0"], source, env)
        configkit = Path(go_cache[0]) / "github.com/danieljustus/symaira-corekit@v0.17.0/configkit/configkit.go"
        assert digest(configkit.read_bytes()) == CONFIGKIT_SHA, "pinned Go configkit changed"
        go = args.go.resolve() if args.go else root / ("oracle.exe" if os.name == "nt" else "oracle")
        if args.go is None:
            run(["go", "build", "-o", str(go), "./cmd/symbrain"], source, env)
        schema_source = repo / "scripts/memory-cli-oracle/schema.go.txt"
        generator = source / "scripts/memory-cli-oracle-schema/main.go"
        generator.parent.mkdir(parents=True)
        generator.write_bytes(schema_source.read_bytes())
        reflected = run(["go", "run", "./scripts/memory-cli-oracle-schema"], source, env).stdout
        expected_fields = [(field["name"], field["kind"]) for field in json.loads(reflected)]
        records = []
        if args.family in ("all", "arguments"):
            arguments(go, rust, records)
        if args.family in ("all", "configuration"):
            configuration(go, rust, repo, records, expected_fields)
        if args.family in ("all", "seeded_reads"):
            seeded_reads(go, rust, records)
        if args.family in ("all", "configured_search"):
            configured_search(go, rust, records)
        if args.family in ("all", "database_paths"):
            records.extend(path_cases.run(go, rust, output, snapshot, isolated_env))
        if args.family in ("all", "corrupt_reads"):
            records.extend(corrupt_reads.run(go, rust, output, snapshot, isolated_env))
        candidate_files = set(run(["git", "ls-files", "--cached", "--others", "--exclude-standard",
                                   "rust/symbrain-cli", "rust/symbrain-memory", "Cargo.toml", "Cargo.lock",
                                   "scripts/memory-cli-oracle", ".github/workflows/memory-cli-native.yml"], repo)
                              .stdout.decode().splitlines())
        report = dict(
            go_oracle_revision=ORACLE, go_binary_sha256=digest(go.read_bytes()),
            rust_binary_sha256=digest(rust.read_bytes()), corekit_configkit_sha256=CONFIGKIT_SHA,
            reflected_go_schema_sha256=digest(reflected), known_config_fields=len(expected_fields),
            schema_generator_sha256=digest(schema_source.read_bytes()),
            immutable_go_cli_source_sha256={str(path.relative_to(source)): digest(path.read_bytes())
                for path in (source / "cmd/symbrain").glob("*memory*.go")},
            candidate_source_sha256={name: digest((repo / name).read_bytes()) for name in sorted(candidate_files)},
            candidate_revision=run(["git", "rev-parse", "HEAD"], repo).stdout.decode().strip(),
            candidate_dirty=bool(run(["git", "status", "--porcelain"], repo).stdout),
            go_version=run(["go", "version"], repo).stdout.decode().strip(),
            operator_home_used=False,
            fallback_available=any(record.get("fallback_available", False) for record in records),
            cases=len(records), passed=sum(record["match"] for record in records), records=records)
        if args.report:
            args.report.write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps({key: value for key, value in report.items()
                          if key not in ("records", "candidate_source_sha256", "immutable_go_cli_source_sha256")}, indent=2))
        assert report["cases"] == report["passed"], "actual Go/Rust output or database state differs"


if __name__ == "__main__":
    main()
