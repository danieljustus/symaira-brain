"""Actual-process default/legacy path choices under disposable XDG/HOME roots."""
import base64
import json
from pathlib import Path
import sqlite3
import tempfile


def run(go, rust, output, snapshot, isolated_env):
    cases = [
        ("relative-data-home", "relative", ["home-current", "relative-current"], None),
        ("absolute-xdg-legacy", "absolute", ["xdg-legacy", "home-legacy"], None),
        ("absolute-both-prefer-current", "absolute", ["xdg-current", "xdg-legacy"], None),
        ("absolute-current-file-legacy-directory", "absolute", ["xdg-legacy"], "xdg-current"),
        ("absolute-legacy-file-current-directory", "absolute", ["xdg-current"], "xdg-legacy"),
        ("unset-current-before-legacy", "unset", ["home-current", "home-legacy"], None),
        ("empty-home-legacy-current-file", "empty", ["home-legacy"], "home-current"),
    ]
    records = []
    for index, (name, mode, choices, regular) in enumerate(cases):
        with tempfile.TemporaryDirectory(prefix="memory-cli-paths-") as temporary:
            root = Path(temporary)
            env = isolated_env(root)
            directories = {
                "home-current": root / ".local/share/symbrain/memory",
                "home-legacy": root / ".local/share/symmemory",
                "xdg-current": root / "data/symbrain/memory",
                "xdg-legacy": root / "data/symmemory",
                "relative-current": root / "relative-data/symbrain/memory",
            }
            for label in choices:
                path = directories[label] / "default.db"
                assert output(go, ["list", "--db", str(path)], root, env)["exit"] == 0
                with sqlite3.connect(path) as database:
                    database.execute(
                        "INSERT INTO memories(id,content,scope,kind,created_at,updated_at,metadata,embedding) "
                        "VALUES(?,?,'global','reference','2000-01-01 00:00:00 +0000 UTC',"
                        "'2000-01-01 00:00:00 +0000 UTC','{}',X'')", (label, label))
            if regular:
                path = directories[regular]
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"ordinary file, not a namespace directory")
            config = root / "config/symmemory/config.toml"
            config.parent.mkdir(parents=True)
            # Activate the newly ported config path without overriding the DB.
            config.write_bytes(b'ollama.model="fixture-only"\n')
            if mode == "relative":
                env["XDG_DATA_HOME"] = "relative-data"
            elif mode == "unset":
                env.pop("XDG_DATA_HOME")
            elif mode == "empty":
                env["XDG_DATA_HOME"] = ""
            before = snapshot(root)
            first, second = (output(binary, ["list", "--json"], root, env) for binary in (go, rust))
            unchanged = before == snapshot(root)
            selected = json.loads(base64.b64decode(first["stdout"])) if first["exit"] == 0 else []
            records.append(dict(family="database_paths", index=index, name=name, xdg_mode=mode,
                                match=first == second and first["exit"] == 0 and len(selected) == 1 and unchanged,
                                database_state_unchanged=unchanged, selected_id=selected[0]["id"] if selected else None,
                                go=first, rust=second))
    return records
