import base64
import json
import os
import subprocess
import sys
import unittest

import oracle


class OracleFixtureTests(unittest.TestCase):
    def test_fixture_is_pinned_and_complete(self):
        fixture = json.loads(oracle.FIXTURE.read_text(encoding="utf-8"))
        oracle.verify_binding(fixture)
        self.assertEqual(fixture["case_count"], len(fixture["cases"]))
        self.assertEqual(fixture["case_count"], 8 if os.name == "nt" else 9)
        ids = [case["id"] for case in fixture["cases"]]
        self.assertEqual(len(ids), len(set(ids)))
        self.assertEqual({"darwin", "linux"}, set(fixture["platform_xdg_sources"]))
        self.assertEqual(fixture.get("native_platform"), "windows" if os.name == "nt" else None)

    def test_windows_projection_excludes_only_the_posix_tty_case(self):
        fixture = json.loads(oracle.FIXTURE.read_text(encoding="utf-8"))
        windows = oracle.fixture_for_platform(fixture, windows=True)
        ids = [case["id"] for case in windows["cases"]]
        self.assertEqual(windows["case_count"], 8)
        self.assertNotIn("default-tty-table", ids)
        self.assertEqual(
            [case["id"] for case in fixture["cases"] if not case["tty"]],
            ids,
        )

    def test_fixture_drift_reports_the_first_changed_case_stream(self):
        fixture = json.loads(oracle.FIXTURE.read_text(encoding="utf-8"))
        changed = json.loads(json.dumps(fixture))
        changed["cases"][0]["stdout"]["sha256"] = "different"
        changed["cases"][0]["args"] = ["different"]
        differences = oracle.fixture_differences(fixture, changed)
        self.assertTrue(any("default-pipe-json stdout differs" in item for item in differences))
        self.assertTrue(any("default-pipe-json args differs" in item for item in differences))

    def test_oracle_module_import_does_not_require_posix_terminal_modules(self):
        subprocess.run(
            [
                sys.executable,
                "-c",
                "import sys; sys.modules['pty'] = None; sys.modules['tty'] = None; import oracle",
            ],
            cwd=oracle.HERE,
            check=True,
        )

    def test_byte_metadata_and_one_finding_repeat(self):
        fixture = json.loads(oracle.FIXTURE.read_text(encoding="utf-8"))
        for case in fixture["cases"]:
            for stream in ("stdout", "stderr"):
                data = base64.b64decode(case[stream]["base64"])
                self.assertEqual(case[stream]["bytes"], len(data), case["id"])
                self.assertEqual(case[stream]["sha256"], oracle.sha256(data), case["id"])
        missing = next(case for case in fixture["cases"] if case["id"] == "one-finding-missing-hermes")
        self.assertEqual(missing["repeat"], 3)
        self.assertIn(b"1 finding(s)", base64.b64decode(missing["stderr"]["base64"]))
if __name__ == "__main__":
    unittest.main()
