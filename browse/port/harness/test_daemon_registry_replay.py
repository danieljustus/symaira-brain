"""Negative controls for the sealed registry and process recordings (#772). No binaries needed."""
import copy
import json
from pathlib import Path
import shutil
import tempfile
import unittest
import unittest.mock

import daemon_process_replay as process_replay
import daemon_registry_replay as replay
import daemon_state_key_replay as sealed


class RegistrySealTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.dir = Path(self.temp.name) / "recording"
        shutil.copytree(replay.RECORDING_DIR, self.dir)

    def tearDown(self):
        self.temp.cleanup()

    def test_committed_recordings_are_sealed_and_complete(self):
        recording, prefix = replay.load(self.dir)
        replay.check_registry_recording(recording)
        self.assertEqual(len(prefix["cases"]), 12)

    def test_changed_registry_recording_fails(self):
        path = self.dir / replay.RECORDING
        data = json.loads(path.read_text())
        data["case"]["go"]["missing"][0]["exit"] = 7
        path.write_text(json.dumps(data))
        with self.assertRaisesRegex(sealed.SealError, "changed recording file: " + replay.RECORDING):
            replay.load(self.dir)

    def test_changed_prefix_recording_fails(self):
        path = self.dir / replay.PREFIX_RECORDING
        path.write_text(path.read_text().replace('"exit": 1', '"exit": 2', 1))
        with self.assertRaisesRegex(sealed.SealError, "changed recording file: " + replay.PREFIX_RECORDING):
            replay.load(self.dir)

    def test_unregistered_and_missing_recordings_fail(self):
        (self.dir / "extra-registry.json").write_text("{}")
        with self.assertRaisesRegex(sealed.SealError, "unregistered recording file: extra-registry.json"):
            replay.load(self.dir)
        (self.dir / "extra-registry.json").unlink()
        (self.dir / replay.PREFIX_RECORDING).unlink()
        with self.assertRaisesRegex(sealed.SealError, "missing recording file: " + replay.PREFIX_RECORDING):
            replay.load(self.dir)

    def test_recorded_counts_are_checked(self):
        recording, _ = replay.load(self.dir)
        recording = copy.deepcopy(recording)
        recording["case"]["go"]["cli_edges"]["invalid"].pop()
        with self.assertRaises(AssertionError):
            replay.check_registry_recording(recording)


class PrefixControlsTest(unittest.TestCase):
    def rows(self):
        _, prefix = replay.load(replay.RECORDING_DIR)
        rows = []
        for case in prefix["cases"]:
            go = replay.prefix_comparable(case["go"])
            rows.append({"arguments": case["arguments"], "go": case["go"], "go_projected": go,
                         "rust_projected": copy.deepcopy(go)})
        return rows

    def test_every_alteration_is_rejected(self):
        self.assertEqual([c["rejected"] for c in replay.prefix_controls(self.rows())], [True, True, True])

    def test_root_projection_hides_only_the_private_root(self):
        _, prefix = replay.load(replay.RECORDING_DIR)
        hinted = next(case["go"] for case in prefix["cases"] if "--json" in case["arguments"])
        projected = replay.prefix_comparable(hinted)
        self.assertIn("<owned-root>/state/symbrowse/daemon.log", projected["stdout"])
        self.assertNotIn("/tmp/bk-prefix-", projected["stdout"])
        self.assertIn("owned-prefix", projected["stdout"])

    def test_darwin_layout_projection_is_exact(self):
        root = "/tmp/br-x"
        observed = {"root": root, "a": [f"/private{root}/p", f"{root}/home/Library/Caches/symbrowse/run/s.sock",
                                        f"{root}/home/Library/Caches/symbrowse/sessions/s", f"{root}/other"]}
        with unittest.mock.patch.object(replay.platform, "system", return_value="Darwin"):
            projected = replay.project_platform_paths(observed)
        self.assertEqual(projected["a"], [f"{root}/p", f"{root}/run/symbrowse/s.sock",
                                          f"{root}/cache/symbrowse/sessions/s", f"{root}/other"])
        with unittest.mock.patch.object(replay.platform, "system", return_value="Linux"):
            self.assertIs(replay.project_platform_paths(observed), observed)


class ProcessSealTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.dir = Path(self.temp.name) / "daemon"
        self.dir.mkdir()
        for name in process_replay.PINNED:
            shutil.copy(process_replay.RECORDING_DIR / name, self.dir / name)

    def tearDown(self):
        self.temp.cleanup()

    def test_committed_recordings_are_sealed_and_complete(self):
        recordings = process_replay.verify_seal(self.dir)
        counts = {}
        for name, recording in recordings.items():
            process_replay.check_recording(name, recording)
            counts[name] = recording["total"]
        self.assertEqual(counts, {"linux-process-report.json": 63, "original-linux-process-report.json": 21})

    def test_changed_unregistered_and_missing_recordings_fail(self):
        path = self.dir / "linux-process-report.json"
        path.write_text(path.read_text().replace('"exit": 0', '"exit": 3', 1))
        with self.assertRaisesRegex(process_replay.SealError, "changed recording file: linux-process-report.json"):
            process_replay.verify_seal(self.dir)
        shutil.copy(process_replay.RECORDING_DIR / "linux-process-report.json", path)
        (self.dir / "windows-process-report.json").write_text("{}")
        with self.assertRaisesRegex(process_replay.SealError, "unregistered recording file: windows-process-report.json"):
            process_replay.verify_seal(self.dir)
        (self.dir / "windows-process-report.json").unlink()
        (self.dir / "original-linux-process-report.json").unlink()
        with self.assertRaisesRegex(process_replay.SealError, "missing recording file: original-linux-process-report.json"):
            process_replay.verify_seal(self.dir)

    def test_failed_or_foreign_recording_is_refused(self):
        recording = json.loads((process_replay.RECORDING_DIR / "linux-process-report.json").read_text())
        broken = copy.deepcopy(recording)
        broken["cases"][0]["matches"] = False
        with self.assertRaises(AssertionError):
            process_replay.check_recording("x", broken)
        foreign = copy.deepcopy(recording)
        foreign["supplemental_go_ref"] = "0" * 40
        with self.assertRaises(AssertionError):
            process_replay.check_recording("x", foreign)


if __name__ == "__main__":
    unittest.main()
