"""SKL-007 / #621: validate the added drift fields before projecting Go bytes."""
import hashlib
import json
from pathlib import Path

RENDER_DEVIATIONS = frozenset({
    "skills_status_opencode_managed_symlink", "skills_status_opencode_managed_symlink_json",
    "skills_status_opencode_managed_copy_json", "skills_status_opencode_managed_harness_changed_json",
    "skills_status_opencode_managed_conflict", "skills_status_opencode_managed_conflict_json",
})


def cache_hash(root):
    cached = root / "data/symbrain/skills/rendered/opencode/demo/SKILL.md"
    base = root / "data/symbrain/skills/base/opencode/demo/SKILL.md"
    assert not cached.is_symlink() and cached.is_file()
    digest = hashlib.sha256(cached.read_bytes()).hexdigest()
    assert digest == hashlib.sha256(base.read_bytes()).hexdigest(), "retained render changed from the frozen Go base"
    return digest


def presentation_mode(mode, root):
    installed = root / "home/.config/opencode/skills/demo"
    if mode == "symlink" and installed.is_symlink():
        cached = root / "data/symbrain/skills/rendered/opencode/demo"
        assert installed.is_symlink() and installed.resolve(strict=True) == cached.resolve(strict=True)
        return "linked"
    assert mode in {"symlink", "copy"} and installed.is_dir() and not installed.is_symlink()
    return mode


def legacy_render_view(name: str, go: bytes, rust: bytes, root: Path) -> bytes:
    assert name in RENDER_DEVIATIONS, "unapproved render-report fixture"
    digest = cache_hash(root)
    if not name.endswith("_json"):
        lines = go.decode().splitlines()
        assert len(lines) == 2 and lines[0] == "TARGET\tSKILL\tSTATUS\tMODE\tPATH"
        fields = lines[1].split("\t")
        assert fields[:2] == ["opencode", "demo"] and len(fields) == 5
        assert fields[2] == ("conflict" if name.endswith("_conflict") else "in-sync")
        label = "drift" if fields[2] == "conflict" else "in-sync"
        fields[3] = presentation_mode(fields[3], root)
        expected = lines[0] + "\tRENDER\n" + "\t".join(fields) + "\t" + label + "\n"
        assert rust == expected.encode(), "new table changed beyond the verified mode/render columns"
        return go
    before, after = json.loads(go), json.loads(rust)
    assert set(before) == set(after) == {"installs", "summary"}
    assert len(before["installs"]) == len(after["installs"]) == 1
    old, new = before["installs"][0], after["installs"][0]
    assert old["target"] == "opencode" and old["name"] == "demo"
    assert new["mode"] == presentation_mode(old["mode"], root)
    new["mode"] = old["mode"]
    is_conflict = name.endswith("_conflict_json")
    assert set(new) == set(old) | ({"render_status", "render_drift"} if is_conflict else {"render_status"})
    assert new.pop("render_status") == ("drift" if is_conflict else "in-sync")
    if old.get("drift"):
        assert len(old["drift"]) == 1 and old["drift"][0]["path"] == "SKILL.md"
        drift = old["drift"][0]
        assert digest == drift["base"]
        assert (digest != drift["left"]) == is_conflict
    if is_conflict:
        assert old["status"] == "conflict"
        assert new.pop("render_drift") == [{
            "path": "SKILL.md", "library_hash": drift["left"], "render_hash": digest,
        }]
    assert after == before, "existing install fields/summary changed"
    projected = json.dumps(after, ensure_ascii=False, separators=(",", ":")).encode() + b"\n"
    assert projected == go, "legacy report bytes changed"
    return projected
