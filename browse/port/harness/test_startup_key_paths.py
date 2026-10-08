"""Check assertion diagnostics only; this is not native provider-path evidence."""
import ast
import json
from pathlib import Path
from types import SimpleNamespace
import unittest


class Tests(unittest.TestCase):
    def test_admission_and_safe_diagnostics(self):
        source = Path(__file__).with_name("startup_key_paths.py")
        tree = ast.parse(source.read_bytes())
        assignments = [node for node in ast.walk(tree) if isinstance(node, ast.Assign)
                       and any(isinstance(target, ast.Name) and target.id == "observation"
                               for target in node.targets)]
        assertions = [node for node in ast.walk(tree) if isinstance(node, ast.Assert)
                      and ast.unparse(node.test) == "observation == expected and len(public_queries) == 1"]
        self.assertEqual((len(assignments), len(assertions)), (1, 1))
        body: list[ast.stmt] = [*assignments, *assertions]
        code = compile(ast.Module(body=body, type_ignores=[]), str(source), "exec")
        expected = {"configured": True, "key_source": "symvault", "error": ""}
        marker = "PRIVATE-DIAGNOSTIC-MARKER"
        cases = [("ascii", expected, 1, False), ("unicode-ä", expected, 1, False),
                 ("no-query", expected, 0, True), ("duplicate-query", expected, 2, True),
                 ("fallback", dict(expected, key_source=marker), 1, True),
                 ("timeout", dict(expected, configured=False, error=f"timed out {marker}"), 0, True),
                 ("other-error", dict(expected, error=marker), 1, True)]
        for label, observation, count, rejected in cases:
            with self.subTest(case=label):
                namespace = {"json": json, "expected": expected, "label": label,
                             "public": SimpleNamespace(stdout=json.dumps(observation).encode()),
                             "public_queries": [marker] * count}
                if not rejected:
                    exec(code, namespace)
                    continue
                with self.assertRaises(AssertionError) as failure:
                    exec(code, namespace)
                diagnostic = failure.exception.args[0]
                self.assertEqual(diagnostic, {
                    "case": label, "configured": observation["configured"],
                    "source_matches": observation["key_source"] == expected["key_source"],
                    "has_error": bool(observation["error"]),
                    "deadline_exceeded": "timed out" in observation["error"], "query_count": count,
                })
                self.assertNotIn(marker, json.dumps(diagnostic))


if __name__ == "__main__":
    unittest.main()
