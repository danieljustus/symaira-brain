"""Source checks only: synthetic comparator records, never product processes."""
import ast
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[5]
OUT = Path(__file__).resolve().parent
SCRIPTS = ROOT / "scripts/skills-native-oracle"


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


def main():
    for path in SCRIPTS.glob("*.py"):
        ast.parse(path.read_bytes())
    sys.path.insert(0, str(SCRIPTS))
    cases = load("cases_source", SCRIPTS / "cases.py")
    compare = load("compare_source", SCRIPTS / "compare.py")
    outputs = load("output_source", SCRIPTS / "output_cases.py")
    denied = load("denied_source", SCRIPTS / "library_denied.py")
    old_text = subprocess.check_output(["git", "show", "db9760a3:scripts/skills-native-oracle/cases.py"], cwd=ROOT, text=True)
    old, new = ast.parse(old_text), ast.parse((SCRIPTS / "cases.py").read_bytes())
    unchanged = []
    for a in old.body:
        if isinstance(a, ast.FunctionDef):
            b = next(n for n in new.body if isinstance(n, ast.FunctionDef) and n.name == a.name)
            assert ast.dump(a) == ast.dump(b), a.name
            unchanged.append(a.name)
    root = Path("/owned/not-created/case")
    old_ids = [n for n, _ in cases.cli_cases()] + [n for n, _ in cases.raw_cli_cases()]
    old_ids += [n for n, _, _ in cases.config_cases()] + [n for n, _, _ in cases.mcp_cases(root)]
    old_ids += [n for n, _, _, _ in cases.mcp_config_cases(root)]
    assert len(old_ids) == len(set(old_ids)) == 567
    extra = [n for n, _, _, _ in cases.correction_cases(root)]
    extra += ["mcp-framed-" + n for n, _, _ in cases.raw_argument_cases()]
    extra += [f"library-{s}-{f}" for s in ("empty-entry", "malformed-entry") for f in ("table", "json")]
    extra += denied.required_ids() + outputs.required_ids()
    assert len(set(old_ids + extra)) == len(old_ids + extra)
    names = ["skills_list", "skills_inspect", "skills_validate", "skills_profile_list", "skills_profile_resolve",
             "skills_render_plan", "skills_install", "skills_discover_sources", "skills_history", "skills_restore", "skills_targets_status"]
    actual_names = re.findall(r'"(skills_[a-z_]+)"', (ROOT / "rust/symbrain-mcp/src/raw_skills.rs").read_text())
    assert actual_names == names
    catalog = [{"name": name} for name in names]

    def record(reply, framed, disabled):
        rows = [{"jsonrpc": "2.0", "id": 1, "result": {}},
                {"jsonrpc": "2.0", "id": 2, "result": {"tools": [] if disabled else catalog}},
                {"jsonrpc": "2.0", "id": 3, **reply}]
        encoded = [json.dumps(row).encode() for row in rows]
        raw = b"".join(b"Content-Length: " + str(len(row)).encode() + b"\r\n\r\n" + row for row in encoded) if framed else b"\n".join(encoded) + b"\n"
        return {"exit": 0, "stdout_hex": raw.hex(), "stderr_hex": "", "filesystem": {"files": {}}, "expected_skills": 0 if disabled else 11}

    projections = []
    for framed in (False, True):
        for disabled in (False, True):
            row = record({"error": {"code": -32601, "message": "owned synthetic RPC error"}}, framed, disabled)
            assert compare.matched(row, row, root, True)
            normal = record({"result": {"content": [{"type": "text", "text": "{}"}], "isError": False}}, framed, disabled)
            assert compare.matched(normal, normal, root, True)
            changed = record({"error": {"code": -32602, "message": "owned changed synthetic RPC error"}}, framed, disabled)
            assert not compare.matched(row, changed, root, True)
            projections.append({"framed": framed, "disabled": disabled, "normal_equal": True, "RPC_error_retained": True, "changed_RPC_error_rejected": True, "synthetic": True})
    tree = ast.parse((SCRIPTS / "run.py").read_bytes())
    owner = next(n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == "main")
    append = [n.lineno for n in ast.walk(owner) if isinstance(n, ast.Call) and ast.unparse(n.func) == "report['results'].append"]
    comparison = [n.lineno for n in ast.walk(owner) if isinstance(n, ast.Call) and ast.unparse(n.func) == "matched"]
    assert min(append) < min(comparison)
    changed = subprocess.check_output(["git", "diff", "--name-only", "db9760a3"], cwd=ROOT, text=True).splitlines()
    extra_files = subprocess.check_output(["git", "ls-files", "--others", "--exclude-standard"], cwd=ROOT, text=True).splitlines()
    rust = sorted({name for name in changed + extra_files if name.endswith(".rs")})
    line_counts = {name: len((ROOT / name).read_text().splitlines()) for name in rust}
    assert max(line_counts.values()) < 400, line_counts
    old_version = subprocess.check_output(["git", "show", "db9760a3:rust/symbrain-cli/src/lib.rs"], cwd=ROOT, text=True)
    start = old_version.index("fn run_version(")
    end = old_version.index("\n#[cfg(unix)]\nfn exit_status_code", start)
    original = old_version[start:end].replace("fn run_version(", "pub(super) fn run(", 1)
    assert original.strip() in (ROOT / "rust/symbrain-cli/src/version_cli.rs").read_text()
    source_maps = [{"path": name, "sha256": hashlib.sha256((ROOT / name).read_bytes()).hexdigest(), "bytes": (ROOT / name).stat().st_size} for name in sorted(set(changed + extra_files)) if (ROOT / name).is_file() and not name.startswith(str(OUT.relative_to(ROOT)))]
    result = {"kind": "SOURCE_ONLY; synthetic Python records, not actual product/SDK/Cargo/provider/port proof",
              "original_case_function_ASTs_unchanged": unchanged, "original567_ids": old_ids, "additive_ids_Unix": extra,
              "prepared_Linux_Darwin": len(old_ids + extra), "prepared_Windows": len(old_ids + extra) - 24,
              "original_mutation_controls": 3, "synthetic_comparator_checks": projections,
              "main_pair_append_lines": append, "main_comparison_lines": comparison,
              "Rust_line_counts": line_counts, "version_body_byte_identical": True,
              "Python_ASTs": [str(p.relative_to(ROOT)) for p in SCRIPTS.glob("*.py")],
              "source_maps": source_maps, "actual_products_SDK_compiler_targets_ports": 0}
    (OUT / "static-checks.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({"Python_ASTs": len(result["Python_ASTs"]), "Rust_files": len(rust), "max_lines": max(line_counts.values()), "Unix_prepared": len(old_ids + extra), "Windows_prepared": len(old_ids + extra) - 24, "source_maps": len(source_maps)}))


if __name__ == "__main__":
    main()
