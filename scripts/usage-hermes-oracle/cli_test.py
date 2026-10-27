"""Verify the narrow #933 exception without running any CLI or real credentials."""
import ast
import base64
import copy
import json
from pathlib import Path
from typing import Any
import unittest

source = ast.parse(Path(__file__).with_name("cli.py").read_text())
helper = next(node for node in source.body if isinstance(node, ast.FunctionDef)
              and node.name == "hermes_login_divergence")
namespace: dict[str, Any] = {"json": json, "base64": base64}
exec(compile(ast.Module(body=[helper], type_ignores=[]), "cli.py", "exec"), namespace)
compare = namespace["hermes_login_divergence"]
OLD = "No Nous Portal credentials found — sign in with the Hermes CLI"
NEW = "Hermes Nous login missing/expired — sign in again with the Hermes CLI"


def observed(value):
    text = value if isinstance(value, str) else json.dumps(value)
    return {"exit": 0, "stderr_b64": "", "stdout_b64": base64.b64encode(text.encode()).decode()}


class HermesDivergenceTests(unittest.TestCase):
    def setUp(self):
        self.old = {"providers": [{"id": "nous", "configured": False,
                                 "auth_status": {"status": "missing", "detail": OLD}}]}
        self.new = copy.deepcopy(self.old)
        self.new["providers"][0]["auth_status"] = {"status": "expired", "detail": NEW, "source": "file"}

    def test_only_documented_auth_change(self):
        self.assertTrue(compare(observed(self.old), observed(self.new), "json", True))
        self.assertFalse(compare(observed(self.old), observed(self.new), "json", False))
        self.assertFalse(compare(observed(self.old), observed(self.old), "json", True))

    def test_unrelated_changes_rejected(self):
        for mutation in [lambda r: r.update(schema_version=99),
                         lambda r: r["providers"][0].update(configured=True),
                         lambda r: r["providers"][0].update(error="other"),
                         lambda r: r["providers"][0]["auth_status"].update(source="env")]:
            changed = copy.deepcopy(self.new)
            mutation(changed)
            self.assertFalse(compare(observed(self.old), observed(changed), "json", True))

    def test_exit_and_stderr_still_match(self):
        for patch in [{"exit": 42}, {"stderr_b64": "eA=="}]:
            changed = observed(self.new)
            changed.update(patch)
            self.assertFalse(compare(observed(self.old), changed, "json", True))

    def test_table_exception(self):
        self.assertTrue(compare(observed("nous\t"+OLD), observed("nous\t"+NEW), "table", True))
        self.assertFalse(compare(observed("nous\t"+OLD), observed("other\t"+NEW), "table", True))


if __name__ == "__main__":
    unittest.main()
