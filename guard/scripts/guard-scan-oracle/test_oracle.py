import base64
import json
import unittest

import oracle


class OracleFixtureTests(unittest.TestCase):
    def test_fixture_is_pinned_and_complete(self):
        fixture = json.loads(oracle.FIXTURE.read_text(encoding="utf-8"))
        oracle.verify_binding(fixture)
        self.assertEqual(fixture["case_count"], len(fixture["cases"]))
        self.assertEqual(fixture["case_count"], 9)
        ids = [case["id"] for case in fixture["cases"]]
        self.assertEqual(len(ids), len(set(ids)))
        self.assertEqual({"darwin", "linux"}, set(fixture["platform_xdg_sources"]))

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
