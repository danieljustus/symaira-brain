"""Negative controls for the sealed state-key recording (#772). No binaries needed."""
import json
from pathlib import Path
import shutil
import tempfile
import unittest
import unittest.mock

import daemon_registry as registry
import daemon_state_key_replay as replay


class SealTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.dir = Path(self.temp.name) / "recording"
        shutil.copytree(replay.RECORDING_DIR, self.dir)

    def tearDown(self):
        self.temp.cleanup()

    def seal(self):
        return replay.verify_seal(self.dir)

    def test_committed_recording_is_sealed(self):
        recording = self.seal()
        replay.check_inputs(recording)
        self.assertEqual(len(recording["cases"]), 31)

    def test_changed_recording_fails(self):
        path = self.dir / replay.RECORDING
        data = json.loads(path.read_text())
        data["cases"][0]["go"]["exit"] = 7
        path.write_text(json.dumps(data))
        with self.assertRaisesRegex(replay.SealError, "changed recording file: " + replay.RECORDING):
            self.seal()

    def test_unregistered_recording_fails(self):
        (self.dir / "extra-recording.json").write_text("{}")
        with self.assertRaisesRegex(replay.SealError, "unregistered recording file: extra-recording.json"):
            self.seal()

    def test_nested_unregistered_recording_fails(self):
        (self.dir / "nested").mkdir()
        (self.dir / "nested" / "symaira-state-key-final-gate.json").write_text("{}")
        with self.assertRaisesRegex(replay.SealError, "unregistered recording file: nested/"):
            self.seal()

    def test_symlinked_recordings_fail(self):
        outside = Path(self.temp.name) / "outside"
        outside.mkdir()
        (outside / "unregistered.json").write_text("{}")
        (self.dir / "linked-dir").symlink_to(outside, target_is_directory=True)
        with self.assertRaisesRegex(replay.SealError, "symlink in recording directory"):
            self.seal()
        (self.dir / "linked-dir").unlink()
        target = self.dir / replay.RECORDING
        moved = outside / replay.RECORDING
        target.rename(moved)
        target.symlink_to(moved)
        with self.assertRaisesRegex(replay.SealError, "symlink in recording directory"):
            self.seal()

    def test_unexpected_keychain_query_fails_on_darwin(self):
        recording = self.seal()
        observation = json.loads(json.dumps(recording["cases"][0]["go"]))
        observation["queries"] = [{"name": "security", "pid": 1, "arguments": ["find-generic-password", "-s", "other"]}]
        with unittest.mock.patch.object(replay.platform, "system", return_value="Darwin"):
            with self.assertRaisesRegex(AssertionError, "unexpected keychain queries"):
                replay.comparable(observation)
        observation["queries"][0]["arguments"] = replay.KEYCHAIN_LOOKUP
        with unittest.mock.patch.object(replay.platform, "system", return_value="Darwin"):
            self.assertEqual(replay.comparable(observation)["queries"], [])

    def test_missing_recording_fails(self):
        (self.dir / replay.RECORDING).unlink()
        with self.assertRaisesRegex(replay.SealError, "missing recording file"):
            self.seal()

    def test_edited_retention_fails(self):
        retention = self.dir / replay.RETENTION
        data = json.loads(retention.read_text())
        data["files_sha256"]["extra-recording.json"] = "0" * 64
        retention.write_text(json.dumps(data))
        with self.assertRaisesRegex(replay.SealError, "differs from the pinned seal"):
            self.seal()

    def test_reseal_requires_new_pin(self):
        # A consistent re-seal still fails until the pin itself is reviewed.
        (self.dir / "extra-recording.json").write_text("{}")
        retention = self.dir / replay.RETENTION
        data = json.loads(retention.read_text())
        data["files_sha256"]["extra-recording.json"] = registry.process.digest(self.dir / "extra-recording.json")
        retention.write_text(json.dumps(data))
        with self.assertRaises(replay.SealError):
            self.seal()
        replay.verify_seal(self.dir, registry.process.digest(retention))

    def test_drifted_inputs_fail(self):
        recording = self.seal()
        recording["cases"][0]["input"]["key"] = "changed"
        with self.assertRaisesRegex(AssertionError, "case inputs drifted"):
            replay.check_inputs(recording)


if __name__ == "__main__":
    unittest.main()
