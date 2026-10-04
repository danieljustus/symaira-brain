import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from skills_render_contract import legacy_render_view


class RenderContractControls(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "home/.config/opencode/skills/demo").mkdir(parents=True)
        path = self.root / "data/symbrain/skills/rendered/opencode/demo/SKILL.md"
        path.parent.mkdir(parents=True)
        path.write_bytes(b"cached fixture\n")
        base = self.root / "data/symbrain/skills/base/opencode/demo/SKILL.md"
        base.parent.mkdir(parents=True)
        base.write_bytes(path.read_bytes())
        self.render_hash = hashlib.sha256(path.read_bytes()).hexdigest()
        self.library_hash = hashlib.sha256(b"fresh library fixture\n").hexdigest()
        self.before = {"installs": [{"target": "opencode", "name": "demo", "mode": "copy",
            "status": "conflict", "drift": [{"path": "SKILL.md", "base": self.render_hash,
            "left": self.library_hash, "right": "harness-hash"}]}], "summary": {"conflict": 1}}
        self.after = copy.deepcopy(self.before)
        self.after["installs"][0].update(render_status="drift", render_drift=[{
            "path": "SKILL.md", "library_hash": self.library_hash, "render_hash": self.render_hash}])

    def encoded(self, value):
        return json.dumps(value, separators=(",", ":")).encode() + b"\n"

    def check(self, value):
        return legacy_render_view("skills_status_opencode_managed_conflict_json",
            self.encoded(self.before), self.encoded(value), self.root)

    def test_projects_only_after_complete_validation(self):
        self.assertEqual(self.check(self.after), self.encoded(self.before))

    def test_rejects_wrong_status_hash_paths_and_extra_fields(self):
        for field, value in (("render_status", "in-sync"), ("status", "in-sync"),
                             ("render_error", "hidden error")):
            changed = copy.deepcopy(self.after)
            changed["installs"][0][field] = value
            with self.subTest(field=field), self.assertRaises(AssertionError): self.check(changed)
        for field in ("library_hash", "render_hash", "path"):
            changed = copy.deepcopy(self.after)
            changed["installs"][0]["render_drift"][0][field] = "mutated"
            with self.subTest(field=field), self.assertRaises(AssertionError): self.check(changed)

    def test_rejects_changed_cached_bytes_or_summary(self):
        changed = copy.deepcopy(self.after)
        changed["summary"]["conflict"] = 2
        with self.assertRaises(AssertionError): self.check(changed)
        (self.root / "data/symbrain/skills/rendered/opencode/demo/SKILL.md").write_bytes(b"mutated")
        with self.assertRaises(AssertionError): self.check(self.after)

    def test_rejects_unlisted_cases_and_missing_extensions(self):
        with self.assertRaises(AssertionError): self.check(self.before)
        with self.assertRaises(AssertionError):
            legacy_render_view("another-case", self.encoded(self.before), self.encoded(self.after), self.root)


if __name__ == "__main__":
    unittest.main()
