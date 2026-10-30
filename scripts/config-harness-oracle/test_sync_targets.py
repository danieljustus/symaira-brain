"""Guard the #909 corpus, recording and nonblocking filesystem observations."""
import json
import os
from pathlib import Path
import stat
import tempfile
import unittest

import project_cases
import replay


@unittest.skipUnless(os.name == "posix", "POSIX target fixtures")
class SyncTargetTests(unittest.TestCase):
    def test_corpus_covers_each_target_and_output_mode(self):
        cases = [case for case in replay.project_corpus_cases()
                 if case.name.startswith("cfgh-sync-target-")]
        self.assertEqual(len(cases), 16)
        self.assertEqual(len({case.name for case in cases}), 16)
        for label, _ in project_cases.SYNC_TARGET_SHAPES:
            selected = [case for case in cases if case.name.startswith(f"cfgh-sync-target-{label}-")]
            self.assertEqual(len(selected), 4)
            self.assertTrue(all(case.mutating and case.posix_only for case in selected))
            self.assertEqual({("--dry-run" in case.argv, "--json" in case.argv)
                              for case in selected}, {(False, False), (False, True),
                                                      (True, False), (True, True)})

    def test_layout_preserves_links_and_special_targets(self):
        for label, ops in project_cases.SYNC_TARGET_SHAPES:
            with self.subTest(label=label), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                project_cases.apply_layout(root, ops)
                target = root / "CLAUDE.md"
                mode = target.lstat().st_mode
                expected_kind = {"symlink": stat.S_ISLNK, "dangling": stat.S_ISLNK,
                                 "directory": stat.S_ISDIR, "fifo": stat.S_ISFIFO}[label]
                self.assertTrue(expected_kind(mode))
                if label == "dangling":
                    self.assertFalse(target.exists())
                if label == "symlink":
                    self.assertEqual((root / "notes").read_bytes(), project_cases.SEED_INSTRUCTIONS)

    def test_fifo_is_in_manifest_without_reading_it(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            os.mkfifo(root / "CLAUDE.md", 0o600)
            self.assertEqual(replay.get_fs_manifest(root)["CLAUDE.md"], ("fifo", 0o600, b""))

    def test_recorded_fixture_is_complete_and_pinned(self):
        fixture = json.loads((replay.ROOT / "rust/symbrain-cli/tests/fixtures/sync_target_go_shapes.json").read_text())
        self.assertEqual(fixture["go_oracle_ref"], replay.ORACLE_REF)
        cases = [case for case in replay.project_corpus_cases()
                 if case.name.startswith("cfgh-sync-target-")]
        self.assertEqual([case.name for case in cases], [case["name"] for case in fixture["cases"]])
        for case in fixture["cases"]:
            self.assertEqual(case["exit"], 1)
            self.assertIn(b"CLAUDE.md is not a regular file", bytes.fromhex(case["stdout_hex"]))
            if "-fifo-" in case["name"]:
                self.assertIn(["project/CLAUDE.md", "fifo", 0o600, ""], case["manifest"])


if __name__ == "__main__":
    unittest.main()
