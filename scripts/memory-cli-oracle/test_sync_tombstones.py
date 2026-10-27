"""Fail-closed controls for the explicitly qualified #854 oracle defect."""
from copy import deepcopy
from typing import Any
import unittest

from sync_pull_cases import build
from sync_relay_cases import native_matches


class TombstoneDivergenceTests(unittest.TestCase):
    def setUp(self):
        self.case = next(case for case in build() if case["name"] == "delete-missing-id")
        self.go: dict[str, Any] = dict(steps=[dict(exit=0, stdout="output", stderr="", requests=["GET"])],
                       state={"x.db": dict(sync_oplog=[], sqlite_sequence=[], memories=[],
                                           audit_log=[], sync_state=[["remote", "cursor"]])},
                       seeded_state={}, tree=None)
        self.native = deepcopy(self.go)
        self.native["state"]["x.db"]["sync_oplog"] = [
            [1, "delete", "nope", "2026-10-26 02:00:00 +0000 UTC"]]
        self.native["state"]["x.db"]["sqlite_sequence"] = [["sync_oplog", 1]]

    def test_only_exact_retained_remote_event_is_accepted(self):
        self.assertTrue(native_matches(self.case, self.go, self.native))
        self.assertFalse(native_matches(self.case, self.go, self.go))
        self.assertEqual(self.go["state"]["x.db"]["sync_oplog"], [])
        for column, value in enumerate([2, "upsert", "other", "wall-clock"]):
            changed = deepcopy(self.native)
            changed["state"]["x.db"]["sync_oplog"][0][column] = value
            self.assertFalse(native_matches(self.case, self.go, changed))

    def test_other_observations_are_still_exact(self):
        for key in ["steps", "seeded_state", "tree"]:
            changed = deepcopy(self.native)
            changed[key] = "changed"
            self.assertFalse(native_matches(self.case, self.go, changed))
        for key in ["memories", "audit_log", "sync_state", "sqlite_sequence"]:
            changed = deepcopy(self.native)
            changed["state"]["x.db"][key] = [["changed"]]
            self.assertFalse(native_matches(self.case, self.go, changed))
        self.assertFalse(native_matches(dict(name="other"), self.go, self.native))
        self.assertTrue(native_matches(dict(name="other"), self.go, self.go))

    def test_exception_cannot_spread_or_hide_an_oracle_repin(self):
        marked = [case["name"] for case in build() if case.get("known_go_defect")]
        self.assertEqual(marked, ["delete-missing-id"])
        with self.assertRaises(AssertionError):
            native_matches(dict(self.case, name="other"), self.go, self.native)
        with self.assertRaises(AssertionError):
            native_matches(self.case, self.native, self.native)


if __name__ == "__main__":
    unittest.main()
