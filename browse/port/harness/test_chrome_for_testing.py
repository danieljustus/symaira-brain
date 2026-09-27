import tempfile
import unittest
import zipfile
from pathlib import Path

from chrome_for_testing import extract_safely


class ChromeForTestingArchiveTests(unittest.TestCase):
    def test_extracts_archive_entries_inside_destination(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "chrome.zip"
            destination = root / "chrome"
            destination.mkdir()
            with zipfile.ZipFile(archive, "w") as bundle:
                bundle.writestr("chrome-linux64/chrome", "fixture")

            extract_safely(archive, destination)

            self.assertEqual((destination / "chrome-linux64" / "chrome").read_text(), "fixture")

    def test_rejects_archive_path_traversal_before_extracting(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "chrome.zip"
            destination = root / "chrome"
            destination.mkdir()
            with zipfile.ZipFile(archive, "w") as bundle:
                bundle.writestr("../escape", "outside")

            with self.assertRaisesRegex(RuntimeError, "escapes destination"):
                extract_safely(archive, destination)

            self.assertFalse((root / "escape").exists())


if __name__ == "__main__":
    unittest.main()
