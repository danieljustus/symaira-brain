#!/usr/bin/env python3
"""Keep the executable candidate selector aligned with the rollback ledger."""
from __future__ import annotations

import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import verify


ROOT = Path(__file__).resolve().parents[3]
MATRIX = ROOT / "browse/port/release/rollback-matrix.json"


class RollbackMatrixTests(unittest.TestCase):
    def test_each_declared_state_matches_executable_selector(self) -> None:
        matrix = json.loads(MATRIX.read_text(encoding="utf-8"))
        self.assertEqual(matrix["schema_version"], 1)
        self.assertEqual(matrix["default"], "go")
        self.assertGreaterEqual(len(matrix["rows"]), 7)

        for row in matrix["rows"]:
            with self.subTest(row=row):
                go_available = row["go"] == "valid"
                rust_available = row["rust"] != "unavailable"
                rust_integrity_ok = row["rust"] != "integrity_failure"
                requested = None if row["request"] == "unset" else row["request"]
                if row["result"] == "block":
                    with self.assertRaises(verify.GateError):
                        verify.select_implementation(
                            requested,
                            go_available=go_available,
                            rust_available=rust_available,
                            rust_integrity_ok=rust_integrity_ok,
                        )
                else:
                    selected = verify.select_implementation(
                        requested,
                        go_available=go_available,
                        rust_available=rust_available,
                        rust_integrity_ok=rust_integrity_ok,
                    )
                    self.assertEqual(selected, row["result"])


if __name__ == "__main__":
    unittest.main()
