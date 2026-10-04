"""Prepared real CLI/MCP cases, without an executed acceptance receipt."""
import os
OMIT_ARGS = object()
TARGETS = ("opencode", "claude", "codex", "hermes", "antigravity", "openclaw")


def cli_cases():
    cases = []
    for verb in ("list", "status", "targets", "log", "sync", "doctor"):
        forms = [[], ["--help"], ["--bogus"], ["--scope"],
                 ["--target"], ["--scope=user"], ["--scope=project"],
                 ["--scope=invalid"], ["--target=invalid"],
                 ["--target=opencode", "--scope=project"],
                 ["positional", "--bogus"], ["--", "--bogus"]]
        if verb == "sync":
            forms += [["--dry-run"], ["--dry-run=0"], ["--dry-run=invalid"]]
        if verb == "log":
            forms += [["--limit=0x1"], ["-l=0b1"], ["--limit=0o1"],
                      ["--limit=01"], ["--limit=-1"], ["--limit=1_0"],
                      ["--limit=9223372036854775808"], ["--limit=bad"],
                      ["--skill= demo "], ["--target= opencode "],
                      ["--skill=   "], ["--skill=demo", "--limit=1"]]
        for index, flags in enumerate(forms):
            for output in ([], ["--json"], ["--output=table"]):
                argv = ["skills", verb, *flags, *output]
                # Every write is an intentional case, with an independently
                # restored owned fixture and complete filesystem result.
                cases.append((f"{verb}-{index}-{len(output)}-{'j' if output == ['--json'] else 't'}", argv))
    for target in TARGETS:
        for verb in ("status", "sync"):
            cases.append((f"{verb}-{target}", ["skills", verb, "--target", target, "--json", "--dry-run"]
                          if verb == "sync" else ["skills", verb, "--target", target, "--json"]))
    return cases + [("root-help", ["skills", "--help"]), ("root-missing", ["skills"]),
                    ("root-unknown", ["skills", "help"])]


def mcp_cases(root):
    valid = [
        ("skills_list", {}), ("skills_profile_list", {}),
        ("skills_profile_resolve", {"name": "work"}),
        ("skills_profile_resolve", {"name": "missing"}),
        ("skills_profile_resolve", {"name": "cycle"}),
        ("skills_inspect", {"name": "demo"}),
        ("skills_inspect", {"path": str(root / "data/symbrain/skills/library/demo")}),
        ("skills_validate", {"name": "demo"}),
        ("skills_validate", {"name": "warning"}),
        ("skills_targets_status", {}),
        ("skills_targets_status", {"scope": "project"}),
        ("skills_targets_status", {"scope": "anything"}),
        ("skills_discover_sources", {"paths": [str(root / "sources")]}),
        ("skills_discover_sources", {"paths": [str(root / "missing-source")]}),
        ("skills_history", {"name": "demo", "limit": 1}),
        ("skills_history", {"name": "demo", "limit": 0}),
        ("skills_history", {"name": "not-versioned"}),
        ("skills_restore", {"name": "demo", "rev": "HEAD~1"}),
        ("skills_restore", {"name": "demo", "rev": "HEAD~1", "dry_run": False}),
        ("skills_restore", {"name": "demo", "rev": "not-a-revision"}),
        ("skills_restore", {"name": "demo", "rev": "HEAD~1", "dry_run": False, "sync": True}),
    ]
    for name in ("skills_render_plan", "skills_install"):
        valid += [(name, {"name": "demo"}), (name, {"name": "demo", "dry_run": True}),
                  (name, {"profile": "work", "dry_run": True}),
                  (name, {"profile": "work", "dry_run": False}),
                  (name, {"profile": "missing", "dry_run": True}),
                  (name, {"name": "demo", "target": "unknown"}),
                  (name, {"name": "demo", "scope": "project", "dry_run": True})]
        valid += [(name, {"name": "demo", "target": target, "dry_run": False}) for target in TARGETS]
    cases = [(f"mcp-{index:03}-{name}", name, args) for index, (name, args) in enumerate(valid)]
    typed = {"skills_inspect": "name", "skills_validate": "path",
             "skills_profile_resolve": "name", "skills_targets_status": "scope",
             "skills_discover_sources": "scope", "skills_history": "name",
             "skills_restore": "name", "skills_render_plan": "target", "skills_install": "target"}
    for name, field in typed.items():
        for index, value in enumerate((0, False, [], {}, None)):
            cases.append((f"mcp-type-{name}-{index}", name, {field: value}))
    # These are genuine bytes passed to the MCP child, not json.loads followed
    # by a reconstructed duplicate-free object.
    cases += [("mcp-duplicate-type-order", "skills_history", '{"name":4,"name":"demo"}'),
              ("mcp-duplicate-null", "skills_history", '{"name":"demo","name":null}'),
              ("mcp-profile-selects-before-bundle", "skills_render_plan", '{"profile":"work","path":4}'),
              ("mcp-default-validates-bundle", "skills_render_plan", '{"profile":"","path":4}')]
    tools = ("skills_list", "skills_inspect", "skills_validate", "skills_profile_list",
             "skills_profile_resolve", "skills_render_plan", "skills_install",
             "skills_discover_sources", "skills_history", "skills_restore", "skills_targets_status")
    cases += [(f"mcp-omitted-{name}", name, OMIT_ARGS) for name in tools]
    for name in tools:
        for index, value in enumerate((0, False, [], '"demo"', None)):
            cases.append((f"mcp-whole-type-{name}-{index}", name, value))
    return cases


def config_cases():
    return [(f"config-{variant}-{verb}", ["skills", verb, "--json"], variant)
            for variant in ("global", "project", "environment", "invalid-global",
                            "invalid-project", "zero-global", "ignored-targets")
            for verb in ("list", "status", "targets", "log", "sync", "doctor")]


def raw_cli_cases():
    values = ["\ufffd", "é", "\U0001f600"]
    values += ["\ud800", "\udc00", "\ud800x\udc00"] if os.name == "nt" else [
        os.fsdecode(b"\xff"), os.fsdecode(b"\xe2\x82"), os.fsdecode(b"\xc0\xaf")]
    return [(f"raw-{verb}-{field}-{index}", ["skills", verb, f"--{field}", value, "--json"])
            for verb in ("list", "status", "targets", "log", "doctor")
            for field in ("scope", "target") for index, value in enumerate(values)] + [
                (f"raw-log-skill-{index}", ["skills", "log", "--skill", value, "--json"])
                for index, value in enumerate(values)]


def mcp_config_cases(root):
    return [(f"mcp-config-{variant}-{name}", name, {}, variant)
            for variant in ("global", "project", "environment", "invalid-global",
                            "invalid-project", "zero-global", "ignored-targets")
            for name in ("skills_list", "skills_targets_status", "skills_profile_list")]
