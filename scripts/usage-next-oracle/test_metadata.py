"""Receipt metadata must describe execution, not the host default compiler."""
import os
from pathlib import Path
import unittest
from unittest.mock import patch

from device import execution_metadata


class ExecutionMetadataTests(unittest.TestCase):
    def test_platform_and_compiler_follow_the_oracle_environment(self):
        env = {**os.environ, "GOTOOLCHAIN": "go1.26.7"}
        source = Path(__file__).resolve().parents[2]
        for system, machine in [("Darwin", "arm64"), ("Linux", "x86_64"), ("Windows", "AMD64")]:
            with self.subTest(system=system), \
                 patch("device.platform.system", return_value=system), \
                 patch("device.platform.machine", return_value=machine), \
                 patch("device.subprocess.check_output") as output:
                output.side_effect = [f"go version go1.26.7 {system}/{machine}\n", "rustc fixture\n"]
                result = execution_metadata(env, source)
                self.assertEqual(result["go_sdk"], f"go version go1.26.7 {system}/{machine}")
                self.assertEqual(result["rust_sdk"], "rustc fixture\n")
                self.assertEqual(result["native_platforms"],
                                 f"{system} {machine} execution only; other native platforms require separate acceptance")
                go_call, rust_call = output.call_args_list
                self.assertEqual(go_call.args, (["go", "version"],))
                self.assertIs(go_call.kwargs["env"], env)
                self.assertEqual(go_call.kwargs["cwd"], source)
                self.assertEqual(rust_call.args, (["rustc", "-Vv"],))
                self.assertIs(rust_call.kwargs["env"], env)


if __name__ == "__main__":
    unittest.main()
