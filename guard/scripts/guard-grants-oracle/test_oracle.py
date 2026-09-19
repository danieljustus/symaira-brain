import base64
import json
import unittest

import oracle

oracle.ensure_external_environment()


class GrantsOracleTests(unittest.TestCase):
    def test_fixture_is_pinned_and_complete(self):
        fixture = json.loads(oracle.FIXTURE.read_text(encoding="utf-8"))
        oracle.verify_binding(fixture)
        self.assertEqual(fixture["case_count"], len(fixture["cases"]))
        self.assertEqual(fixture["case_count"], 13)
        ids = [case["id"] for case in fixture["cases"]]
        self.assertEqual(len(ids), len(set(ids)))
        self.assertEqual(ids, [
            "list-empty", "list-ordered", "list-fractional-offset", "revoke-id", "revoke-all",
            "revoke-missing", "malformed", "top-level-null", "legacy-list", "legacy-revoke",
            "legacy-nullable", "string-escaping", "usage-all-with-id",
        ])

    def test_stream_and_state_metadata(self):
        fixture = json.loads(oracle.FIXTURE.read_text(encoding="utf-8"))
        for case in fixture["cases"]:
            for stream in ("stdout", "stderr"):
                data = base64.b64decode(case[stream]["base64"])
                self.assertEqual(case[stream]["bytes"], len(data), case["id"])
                self.assertEqual(case[stream]["sha256"], oracle.sha256(data), case["id"])
            for item in case["state"]["files"]:
                self.assertEqual(item["mode"], "0600", case["id"])
                data = base64.b64decode(item["content"]["base64"])
                self.assertEqual(item["content"]["bytes"], len(data), case["id"])
                self.assertEqual(item["content"]["sha256"], oracle.sha256(data), case["id"])
            for item in case["state"]["directories"]:
                self.assertEqual(item["mode"], "0700", case["id"])
        malformed = next(case for case in fixture["cases"] if case["id"] == "malformed")
        self.assertIn(b"grant: parse", base64.b64decode(malformed["stdout"]["base64"]))
        revoke_all = next(case for case in fixture["cases"] if case["id"] == "revoke-all")
        self.assertIn(b"Revoked 2 grant(s).", base64.b64decode(revoke_all["stdout"]["base64"]))


if __name__ == "__main__":
    unittest.main()
