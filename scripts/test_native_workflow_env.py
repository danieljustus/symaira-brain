"""Executable contracts for native workflow environment handoffs."""
from pathlib import Path
import os
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class NativeWorkflowEnvironmentTests(unittest.TestCase):
    def test_windows_fixture_exports_preserve_values_and_order(self):
        text = (ROOT / ".github/workflows/ci.yml").read_text()
        step = text.split("- name: Generate native Windows oracle fixtures", 1)[1]
        step = step.split("- name:", 1)[0]
        block = step[step.index("          {\n"):]
        block = block[:block.index('} >> "$GITHUB_ENV"') + len('} >> "$GITHUB_ENV"')]
        names = [
            ("XDG", "xdg"), ("GUARD_DOCTOR", "doctor"),
            ("INSTRUCTIONS", "instructions"), ("CLI_TREE", "cli"),
            ("ACTIVITY_CLI", "activity_cli"), ("PROFILE_REMOVE", "profile_remove"),
            ("SKILLS", "skills"), ("SKILLS_INSTALL", "skills_install"),
            ("GUARD_SCAN", "scan"),
        ]
        with tempfile.TemporaryDirectory(prefix="workflow-env-") as directory:
            output = Path(directory) / "github env"
            output.write_text("existing=value\n")
            env = dict(os.environ, GITHUB_ENV=str(output))
            for _, variable in names:
                env[variable + "_fixture"] = f"C:/owned path/{variable}=fixture.json"
            subprocess.run(["bash", "-euo", "pipefail", "-c", block], env=env, check=True)
            expected = "existing=value\n" + "".join(
                f"SYMBRAIN_{name}_ORACLE_FIXTURE={env[variable + '_fixture']}\n"
                for name, variable in names
            )
            self.assertEqual(output.read_text(), expected)

    def test_node_lookup_is_exported_and_failure_is_not_masked(self):
        text = (ROOT / ".github/workflows/memory-http-native.yml").read_text()
        lines = [line.strip() for line in text.splitlines()
                 if line.strip().startswith(("MEMORY_HTTP_NODE=", "export MEMORY_HTTP_NODE"))]
        self.assertEqual(len(lines), 2)
        script = "\n".join(lines)
        for lookup, expected_code in [("printf '/owned node\\n'", 0), ("return 7", 7)]:
            with self.subTest(lookup=lookup):
                probe = f"command() {{ {lookup}; }}\n{script}\nbash -c 'printf %s \"$MEMORY_HTTP_NODE\"'"
                result = subprocess.run(["bash", "-euo", "pipefail", "-c", probe],
                                        capture_output=True, text=True, check=False)
                self.assertEqual(result.returncode, expected_code, result.stderr)
                self.assertEqual(result.stdout, "/owned node" if expected_code == 0 else "")


if __name__ == "__main__":
    unittest.main()
