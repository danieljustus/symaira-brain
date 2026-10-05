"""Owned Skills fixtures; called only when a real runtime slot is allocated."""
import json
import os
from pathlib import Path
import subprocess
import shutil

TARGET_ROOTS = {"opencode": ".config/opencode/skills", "claude": ".claude/skills",
                "codex": ".agents/skills", "hermes": ".hermes/skills/symaira",
                "antigravity": ".gemini/config/skills", "openclaw": ".openclaw/skills"}
PROFILE = '''[profile]
name = "skills-native-764"
[servers.vault]
enabled = false
[servers.memory]
enabled = false
[servers.skills]
enabled = true
[servers.usage]
enabled = false
[audit]
enabled = false
'''


def write(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data.encode() if isinstance(data, str) else data)


def environment(root, git):
    env = {"HOME": str(root / "home"), "USERPROFILE": str(root / "home"),
           "XDG_CONFIG_HOME": str(root / "config"), "XDG_DATA_HOME": str(root / "data"),
           "XDG_CACHE_HOME": str(root / "cache"), "XDG_STATE_HOME": str(root / "state"),
           "TMPDIR": str(root / "tmp"), "TEMP": str(root / "tmp"), "TMP": str(root / "tmp"),
           "PATH": str(Path(git).parent), "SYMBRAIN_GO_BINARY": str(root / "never-go"),
           "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8", "TZ": "UTC",
           "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull,
           "GIT_TERMINAL_PROMPT": "0", "GIT_AUTHOR_DATE": "2020-01-02T03:04:05Z",
           "GIT_COMMITTER_DATE": "2020-01-02T03:04:05Z"}
    if os.name == "nt":
        for key in ("SystemRoot", "windir", "ComSpec", "SystemDrive", "PATHEXT"):
            if key in os.environ:
                env[key] = os.environ[key]
    return env



def fixture_command(argv, cwd, env, ledger):
    row = {"argv": argv, "cwd": str(cwd), "exit": None}
    ledger.append(row)
    try:
        out = subprocess.run(argv, cwd=cwd, env=env, capture_output=True, timeout=20, check=False)
    except subprocess.TimeoutExpired as error:
        row.update(timed_out=True, stdout_hex=(error.stdout or b"").hex(),
                   stderr_hex=(error.stderr or b"").hex())
        raise
    except OSError as error:
        row["launch_error"] = {"type": type(error).__name__, "message": str(error)}
        raise
    row.update(exit=out.returncode, stdout_hex=out.stdout.hex(), stderr_hex=out.stderr.hex())
    return out


def setup(root, git, ledger=None):
    ledger = [] if ledger is None else ledger
    for name in ("home", "config", "data", "cache", "state", "project", "tmp", "sources"):
        (root / name).mkdir(parents=True)
    write(root / "profile.toml", PROFILE)
    library = root / "data/symbrain/skills/library"
    demo = library / "demo"
    write(demo / "SKILL.md", "---\nname: demo\ndescription: bounded skill\nversion: 1\ncategory: work\n---\nfirst body\n")
    write(demo / "notes.md", "owned resource\n")
    env = environment(root, git)
    commands = [[git, "init"], [git, "add", "-A"],
                [git, "-c", "user.name=fixture", "-c", "user.email=fixture@localhost",
                 "-c", "commit.gpgsign=false", "commit", "-m", "import: fixture one"]]
    for argv in commands:
        out = fixture_command(argv, demo, env, ledger)
        if out.returncode:
            raise RuntimeError("owned Git fixture initialization failed")
    write(demo / "SKILL.md", "---\nname: demo\ndescription: bounded skill\nversion: 2\ncategory: work\n---\nsecond body\n")
    for argv in ([git, "add", "-A"], [git, "-c", "user.name=fixture", "-c", "user.email=fixture@localhost",
                                    "-c", "commit.gpgsign=false", "commit", "-m", "update: fixture two"]):
        out = fixture_command(argv, demo, env, ledger)
        if out.returncode:
            raise RuntimeError("owned second Git commit failed")
    write(library / "warning/SKILL.md", "---\nname: warning\n---\nbody\n")
    write(library / "broken/SKILL.md", "missing frontmatter\n")
    write(root / "sources/external/SKILL.md", "---\nname: external\ndescription: source\n---\nbody\n")
    profiles = root / "config/symskills/profiles"
    for name, body in {
        "base": 'name = "base"\n[links.demo]\nskill = "demo"\n',
        "work": 'name = "work"\ninherits = ["base"]\n[links.demo]\nskill = "demo"\nalias = "aliased"\n',
        "missing": 'name = "missing"\n[links.absent]\nskill = "absent"\n',
        "cycle": 'name = "cycle"\ninherits = ["cycle"]\n',
    }.items():
        write(profiles / f"{name}.toml", body)
    events = []
    for target, suffix in TARGET_ROOTS.items():
        installed = root / "home" / suffix / "demo"
        write(installed / "SKILL.md", demo.joinpath("SKILL.md").read_bytes())
        write(installed / ".symskills.json", json.dumps({"schema_version": 1, "managed_by": "symskills",
              "target": target, "name": "demo", "mode": "copy", "installed": "2020-01-02T03:04:05Z",
              "source_hash": "stale-fixture-hash"}))
        events.append({"ts": "2020-01-02T03:04:05.999999999Z", "event": "install", "skill": "demo",
                       "target": target, "mode": "copy", "path": str(installed), "outcome": "ok", "actor": "cli"})
    events += [{"ts": "2020-01-02T03:04:05.000000001Z", "event": "render", "skill": "demo", "outcome": "ok", "actor": "mcp"},
               {"ts": "2020-01-02T03:04:05.999999999Z", "event": "render", "skill": "demo", "outcome": "ok", "actor": "cli"},
               {"ts": "2020-01-02T03:04:05Z", "event": "install", "skill": "\ufffd", "outcome": "ok", "actor": "cli"}]
    write(root / "home/.local/share/symskills/events.jsonl", "\n".join(json.dumps(event) for event in events) + "\n")
    # Fixed future access time avoids relatime updating the last-used fixture
    # between sequential Go/native processes. It is not a product clock mock.
    for path in sorted(root.rglob("*"), reverse=True):
        if not path.is_symlink():
            os.utime(path, ns=(1893456000000000123, 1577934245000000000))
    return env, ledger


def variant(root, env, name, git_fixture=None):
    env = dict(env)
    if name == "skills-disabled":
        write(root / "profile.toml", PROFILE.replace('[servers.skills]\nenabled = true', '[servers.skills]\nenabled = false'))
        return env
    if name == "discovery-edges":
        write(root / "sources/regular", b"ordinary file")
        (root / "sources/empty").mkdir()
        os.symlink("absent", root / "sources/dangling", target_is_directory=True)
        return env
    if name in ["empty-entry", "malformed-entry"]:
        bundle = root / "data/symbrain/skills/library/edge"
        bundle.mkdir()
        if name != "empty-entry":
            write(bundle / "SKILL.md", b"missing frontmatter" if name == "malformed-entry" else b"---\nname: edge\n---\nbody\n")
        return env
    if name.startswith("git-"):
        if git_fixture is None: raise ValueError("bound owned Git fixture required")
        directory = root / "owned-git"
        directory.mkdir()
        target = directory / ("git.exe" if os.name == "nt" else "git")
        shutil.copy2(git_fixture, target)
        env["PATH"] = str(directory)
        env["SKILLS_FIXTURE_REAL_GIT"] = env.pop("SKILLS_ACTUAL_GIT")
        env["SKILLS_FIXTURE_GIT_LEDGER"] = str(root / "tmp/git-provider.jsonl")
        env["SKILLS_FIXTURE_GIT_MODE"] = name.removeprefix("git-")
        return env
    alternate = root / "alternate-library"
    alternate.mkdir()
    quoted = json.dumps(str(alternate))
    global_config = root / "config/symskills/config.toml"
    project_config = root / "project/.symskills.toml"
    if name == "global":
        write(global_config, f"library_dir = {quoted}\n[vcs]\nenabled = false\n")
    elif name == "project":
        write(global_config, 'library_dir = "global-loses"\n')
        write(project_config, f"library_dir = {quoted}\n[vcs]\nenabled = false\n")
    elif name == "environment":
        write(global_config, 'library_dir = "global-loses"\n')
        write(project_config, 'library_dir = "project-loses"\n')
        env["SYMSKILLS_LIBRARY_DIR"] = str(alternate)
        env["SYMSKILLS_VCS_ENABLED"] = "0"
    elif name == "invalid-global":
        write(global_config, "library_dir = true\n")
        env["SYMSKILLS_LIBRARY_DIR"] = str(alternate)
    elif name == "invalid-project":
        write(global_config, f"library_dir = {quoted}\n")
        write(project_config, "library_dir = [\n")
    elif name == "zero-global":
        write(global_config, "library_dir = false\nrender_dir = 0\nbase_dir = 0.0\n[vcs]\nenabled = false\n")
    elif name == "ignored-targets":
        write(global_config, '[[targets]]\nname = "custom"\nskill_root_user = "custom-root"\n[capabilities.codex]\nmcp = true\n')
    else:
        raise ValueError("unknown prepared variant")
    return env
