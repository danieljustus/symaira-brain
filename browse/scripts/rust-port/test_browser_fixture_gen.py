"""Negative controls for the sealed browser recordings (#776). No Go, no browser."""
from pathlib import Path
import shutil
import sys
import tempfile
import unittest

sys.path[:0] = [str(Path(__file__).resolve().parent), str(Path(__file__).resolve().parents[3] / "scripts")]
from external_env import ensure_external_environment  # noqa: E402

# Re-exec this test (not the imported generator) through the external-storage wrapper.
ensure_external_environment(__file__)
import browser_fixture_gen as gen  # noqa: E402


class SealedRecordingTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.dir = Path(self.temp.name)
        for path in gen.FIXTURE_DIR.iterdir():
            shutil.copyfile(path, self.dir / path.name)

    def tearDown(self):
        self.temp.cleanup()

    def verify(self, suite="chrome-full"):
        fixture, sources = ((gen.CHROME_FIXTURE, gen.CHROME_SOURCES) if suite == "chrome-full"
                            else (gen.SAFARI_FIXTURE, gen.SAFARI_SOURCES))
        return gen.verify_sealed(suite, fixture, sources, self.dir)

    def test_committed_recordings_are_sealed(self):
        self.assertEqual(self.verify()["oracle_commit"], gen.ORACLE_COMMIT)
        self.assertEqual(self.verify("safari")["oracle_commit"], gen.ORACLE_COMMIT)

    def test_changed_fixture_fails(self):
        with (self.dir / "chrome-full.json").open("a") as handle:
            handle.write(" ")
        with self.assertRaisesRegex(SystemExit, "fixture differs"):
            self.verify()

    def test_changed_manifest_fails(self):
        with (self.dir / "safari-manifest.json").open("a") as handle:
            handle.write("\n")
        with self.assertRaisesRegex(SystemExit, "pinned seal"):
            self.verify("safari")

    def test_unregistered_recording_fails(self):
        (self.dir / "chrome-full-extra.json").write_text("{}")
        with self.assertRaisesRegex(SystemExit, "unregistered or missing"):
            self.verify()

    def test_missing_recording_fails(self):
        (self.dir / "safari.json").unlink()
        with self.assertRaisesRegex(SystemExit, "unregistered or missing"):
            self.verify("safari")

    def test_record_mode_is_fenced(self):
        with self.assertRaisesRegex(SystemExit, "HISTORICAL RECORD MODE"):
            gen.run_suite("chrome-full", check=False, native=False)


if __name__ == "__main__":
    unittest.main()
