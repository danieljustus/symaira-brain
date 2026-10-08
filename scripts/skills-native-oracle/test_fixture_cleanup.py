"""One executable check for owned read-only fixture cleanup."""
from pathlib import Path
import stat
import subprocess
import tempfile
import unittest

from fixtures import remove_tree


class FixtureCleanup(unittest.TestCase):
    def test_readonly_git_object_is_removed_without_touching_sibling(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "fixture"
            root.mkdir()
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            result = subprocess.run(["git", "-C", str(root), "hash-object", "-w", "--stdin"],
                                    input=b"owned fixture\n", capture_output=True, check=True)
            digest = result.stdout.decode().strip()
            blob = root / ".git/objects" / digest[:2] / digest[2:]
            blob.chmod(stat.S_IREAD)
            sibling = Path(directory) / "retained"
            sibling.write_bytes(b"unchanged\n")
            remove_tree(root)
            self.assertFalse(root.exists())
            self.assertEqual(sibling.read_bytes(), b"unchanged\n")


if __name__ == "__main__":
    unittest.main()
