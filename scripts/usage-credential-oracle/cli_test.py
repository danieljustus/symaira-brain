"""Narrow #930 oracle exception tests; no subprocesses or operator state."""
import ast
import base64
import copy
import json
import pathlib
import unittest
from typing import Any

# Load only the comparison helper, not cli.py's command-line/process driver.
source = ast.parse(pathlib.Path(__file__).with_name("cli.py").read_text())
helper = next(node for node in source.body if isinstance(node, ast.FunctionDef)
              and node.name == "reference_resolution_divergence")
namespace: dict[str, Any] = {"base64": base64, "json": json}
exec(compile(ast.Module(body=[helper], type_ignores=[]), "cli.py", "exec"), namespace)
compare = namespace["reference_resolution_divergence"]


def observation(report):
    return {"exit": 0, "stdout_b64": base64.b64encode(json.dumps(report).encode()).decode(),
            "stderr_b64": ""}


class ReferenceDivergenceTests(unittest.TestCase):
    def setUp(self):
        self.old = {"schema_version": 1, "providers": [{
            "id": "openrouter", "configured": False,
            "auth_status": {"status": "missing", "source": "vault",
                            "detail": "credential resolution failed: resolve symvault://fixture"},
        }]}
        self.fixed = copy.deepcopy(self.old)
        self.fixed["providers"][0].update(
            configured=True, error=self.old["providers"][0]["auth_status"]["detail"])

    def test_accepts_only_the_documented_promotion(self):
        self.assertTrue(compare(observation(self.old), observation(self.fixed)))
        self.assertFalse(compare(observation(self.old), observation(self.old)))
        self.assertFalse(compare(observation(self.fixed), observation(self.fixed)))

    def test_rejects_changes_to_every_unrelated_field(self):
        mutations = [
            lambda r: r.update(schema_version=2),
            lambda r: r["providers"][0].update(id="other"),
            lambda r: r["providers"][0].update(configured=False),
            lambda r: r["providers"][0].update(error="secret or unrelated failure"),
            lambda r: r["providers"][0]["auth_status"].update(source="env"),
            lambda r: r["providers"].append({"id": "extra"}),
        ]
        for mutate in mutations:
            changed = copy.deepcopy(self.fixed)
            mutate(changed)
            self.assertFalse(compare(observation(self.old), observation(changed)))

    def test_rejects_exit_stderr_and_invalid_json(self):
        for patch in [{"exit": 42}, {"stderr_b64": "ZXJyb3I="}, {"stdout_b64": "eA=="}]:
            changed = observation(self.fixed)
            changed.update(patch)
            self.assertFalse(compare(observation(self.old), changed))

    def test_never_promotes_an_absent_credential(self):
        self.old["providers"][0]["auth_status"]["detail"] = "no API key configured"
        self.assertFalse(compare(observation(self.old), observation(self.fixed)))


if __name__ == "__main__":
    unittest.main()
