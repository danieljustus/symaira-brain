"""Real Git fixture stat checks; raw index bytes remain strict."""
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

from fixtures import remove_tree, setup


class GitFixtureTests(unittest.TestCase):
    def test_cloned_index_stays_identical_and_content_changes_are_detected(self):
        git = shutil.which("git")
        if git is None:
            self.fail("real Git required for the native fixture")
        with tempfile.TemporaryDirectory(prefix="skills-git-stat-") as directory:
            owned = Path(directory)
            root, retained = owned / "case", owned / "retained"
            env, ledger = setup(root, git)
            self.assertEqual(ledger[-1]["argv"], [git, "update-index", "--refresh"])
            shutil.copytree(root, retained)
            index = Path("data/symbrain/skills/library/demo/.git/index")
            expected = (retained / index).read_bytes()
            for _ in range(2):
                remove_tree(root)
                shutil.copytree(retained, root)
                demo = root / index.parent.parent
                for args, output in [("status --porcelain", b""),
                                     ("diff --name-only --no-color HEAD~1", b"SKILL.md\n")]:
                    process = subprocess.run([git, *args.split()], cwd=demo, env=env,
                                             capture_output=True, timeout=20)
                    self.assertEqual((process.returncode, process.stdout, process.stderr),
                                     (0, output, b""))
                    self.assertEqual((root / index).read_bytes(), expected)
                (demo / "notes.md").write_bytes(b"changed resource\n")
                process = subprocess.run([git, "status", "--porcelain"], cwd=demo,
                                         env=env, capture_output=True, timeout=20)
                self.assertEqual(process.returncode, 0)
                self.assertEqual(process.stderr, b"")
                self.assertIn(b"notes.md", process.stdout)


if __name__ == "__main__":
    unittest.main()
