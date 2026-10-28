"""Unit controls for family-scoped immutable revision and provenance binding."""
import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import replay


class FamilyRevisionTests(unittest.TestCase):
    def invoke(self, family):
        builds = []

        def fake_build(args, repo, rust, revision, families):
            builds.append((revision, families))
            # Deliberate unit-test fixtures, never subprocess acceptance evidence.
            return dict(go_oracle_revision=revision, go_binary_sha256=revision + "-binary",
                        immutable_go_cli_source_sha256={"cmd/memory.go": revision + "-source"},
                        corekit_configkit_sha256=revision + "-configkit",
                        records=[dict(family=name, match=True) for name in sorted(families)],
                        cases=len(families), passed=len(families))

        with tempfile.TemporaryDirectory() as temporary:
            report = Path(temporary) / "report.json"
            with patch.object(replay, "replay_revision", side_effect=fake_build), \
                    patch("sys.argv", ["replay.py", "--family", family, "--report", str(report)]), \
                    contextlib.redirect_stdout(io.StringIO()):
                replay.main()
            return builds, json.loads(report.read_text())

    def test_all_keeps_non_sync_pin_and_binds_both_source_and_binary_sets(self):
        builds, report = self.invoke("all")
        self.assertEqual([revision for revision, _ in builds], [replay.ORACLE, replay.SYNC_ORACLE])
        self.assertEqual(builds[1][1], {"sync_preflight", "sync_relay"})
        self.assertEqual(len(builds[0][1]), 7)
        self.assertEqual((report["cases"], report["passed"]), (9, 9))
        provenance = {item["go_oracle_revision"]: item for item in report["oracle_provenance"]}
        for record in report["records"]:
            revision = replay.SYNC_ORACLE if record["family"].startswith("sync_") else replay.ORACLE
            self.assertEqual(record["go_oracle_revision"], revision)
            self.assertEqual(provenance[revision]["go_binary_sha256"], revision + "-binary")
            self.assertEqual(provenance[revision]["immutable_go_cli_source_sha256"],
                             {"cmd/memory.go": revision + "-source"})
            self.assertEqual(provenance[revision]["corekit_configkit_sha256"], revision + "-configkit")

    def test_each_family_builds_only_its_own_oracle(self):
        for family in ("sync_preflight", "sync_relay", "arguments", "store_creation"):
            with self.subTest(family=family):
                builds, report = self.invoke(family)
                revision = replay.SYNC_ORACLE if family.startswith("sync_") else replay.ORACLE
                self.assertEqual(builds, [(revision, {family})])
                self.assertEqual(report["go_oracle_revision"], revision)

    def test_single_prebuilt_binary_cannot_claim_two_revisions(self):
        with patch("sys.argv", ["replay.py", "--go", "one-binary"]), \
                patch.object(replay, "replay_revision") as build, \
                contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit) as raised:
                replay.main()
            self.assertEqual(raised.exception.code, 2)
            build.assert_not_called()


if __name__ == "__main__":
    unittest.main()
