"""Regression checks for external runner temp roots and confinement.

Run with: python3 -m unittest discover -s scripts -p test_external_env.py -v
The shell unit tests inject the platform/mount probe; the native test performs
an actual AF_UNIX bind on the configured macOS NVMe, without probe substitution.
"""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


WRAPPER = Path(__file__).resolve().with_name("run-external-env.sh")
NVME = Path("/Volumes/1TB_NVMe_SN850X")
OUTPUT_PATHS = (
    "TMPDIR", "TMP", "TEMP", "GOTMPDIR", "GOPATH", "GOTELEMETRYDIR",
    "GOCACHE", "GOMODCACHE", "CARGO_HOME", "CARGO_TARGET_DIR",
    "PYTHONPYCACHEPREFIX", "SYMAIRA_EXTERNAL_RUNTIME_ROOT",
    "GUARD_DECIDE_EVIDENCE", "GUARD_REPAIR_OUTPUT",
)
ENV_JSON = "import json,os; print(json.dumps(dict(os.environ)))"


def clean_environment():
    env = os.environ.copy()
    for name in (*OUTPUT_PATHS, "CI", "RUNNER_TEMP", "SYMAIRA_EXTERNAL_BASE",
                 "SYMAIRA_EXTERNAL_ENV_READY"):
        env.pop(name, None)
    return env


class ExternalEnvironmentTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="external-env-")
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name).resolve()
        self.volume = self.root / "nvme"
        self.volume.mkdir()
        self.runtime = self.volume / "run"
        self.base = self.volume / "deep" / "build" / "cache"
        self.env = clean_environment()
        self.env.update(SYMAIRA_EXTERNAL_BASE=str(self.base),
                        SYMAIRA_EXTERNAL_RUNTIME_ROOT=str(self.runtime))

    def run_shell(self, platform="Darwin", mounted=True):
        # Only discovery is substituted. The production validator, symlink
        # resolution, exports and directory creation all execute unchanged.
        script = '''
source "$1"
SYMAIRA_NVME_ROOT="$2"
uname() { printf '%s\\n' "$TEST_PLATFORM"; }
df() { printf 'Filesystem blocks used available capacity Mounted on\\n';
       printf 'testfs 1 0 1 0%% %s\\n' "$TEST_MOUNT"; }
external_env
external_env_ready
exec "$3" -c "$4"
'''
        env = self.env.copy()
        env.update(TEST_PLATFORM=platform,
                   TEST_MOUNT=str(self.volume if mounted else self.root))
        return subprocess.run(
            ["bash", "-eu", "-c", script, "test-external-env", str(WRAPPER),
             str(self.volume), sys.executable, ENV_JSON],
            env=env, capture_output=True, text=True, timeout=30,
        )

    def test_darwin_temp_uses_runtime_not_build_cache(self):
        result = self.run_shell()
        self.assertEqual(result.returncode, 0, result.stderr)
        env = json.loads(result.stdout)
        for name in ("TMPDIR", "TMP", "TEMP"):
            self.assertEqual(env[name], str(self.runtime))
        self.assertEqual(env["GOTMPDIR"], str(self.base / "go-tmp"))
        self.assertEqual(env["GOCACHE"], str(self.base / "go-cache"))
        for name in OUTPUT_PATHS:
            self.assertTrue(Path(env[name]).is_relative_to(self.volume), name)
        self.assertEqual(env["SYMAIRA_EXTERNAL_ENV_READY"], "1")

    def test_rejects_runtime_outside_volume(self):
        self.env["SYMAIRA_EXTERNAL_RUNTIME_ROOT"] = str(self.root / "outside")
        result = self.run_shell()
        self.assertEqual(result.returncode, 2)
        self.assertIn("SYMAIRA_EXTERNAL_RUNTIME_ROOT must resolve under", result.stderr)
        self.assertFalse((self.root / "outside").exists())
        self.assertEqual(result.stdout, "")

    def test_rejects_runtime_symlink_escape(self):
        outside = self.root / "outside"
        outside.mkdir()
        self.runtime.symlink_to(outside, target_is_directory=True)
        result = self.run_shell()
        self.assertEqual(result.returncode, 2)
        self.assertIn("SYMAIRA_EXTERNAL_RUNTIME_ROOT must resolve under", result.stderr)
        self.assertEqual(result.stdout, "")
        self.assertEqual(list(outside.iterdir()), [])

    def test_rejects_runtime_parent_traversal(self):
        self.env["SYMAIRA_EXTERNAL_RUNTIME_ROOT"] = str(self.volume / ".." / "escape")
        result = self.run_shell()
        self.assertEqual(result.returncode, 2)
        self.assertIn("SYMAIRA_EXTERNAL_RUNTIME_ROOT must resolve under", result.stderr)
        self.assertFalse((self.root / "escape").exists())

    def test_rejects_external_build_base_escape(self):
        self.env["SYMAIRA_EXTERNAL_BASE"] = str(self.root / "outside")
        result = self.run_shell()
        self.assertEqual(result.returncode, 2)
        self.assertIn("SYMAIRA_EXTERNAL_BASE must resolve under", result.stderr)
        self.assertFalse((self.root / "outside").exists())

    def test_missing_mount_fails_without_creating_paths(self):
        result = self.run_shell(mounted=False)
        self.assertEqual(result.returncode, 2)
        self.assertIn("external build storage is not mounted", result.stderr)
        self.assertFalse(self.base.exists())
        self.assertFalse(self.runtime.exists())

    def test_ci_keeps_runner_temp(self):
        runner_temp = self.root / "ci-temp"
        runner_temp.mkdir()
        self.env.update(CI="1", RUNNER_TEMP=str(runner_temp))
        result = self.run_shell()
        self.assertEqual(result.returncode, 0, result.stderr)
        env = json.loads(result.stdout)
        for name in ("TMPDIR", "TMP", "TEMP"):
            self.assertEqual(env[name], str(runner_temp))
        self.assertFalse(self.base.exists())

    def test_non_darwin_keeps_temp_environment(self):
        self.env.update(TMPDIR=str(self.root), TMP=str(self.root), TEMP=str(self.root))
        result = self.run_shell(platform="Linux")
        self.assertEqual(result.returncode, 0, result.stderr)
        env = json.loads(result.stdout)
        for name in ("TMPDIR", "TMP", "TEMP"):
            self.assertEqual(env[name], str(self.root))
        self.assertFalse(self.base.exists())

    @unittest.skipUnless(sys.platform == "darwin" and not os.environ.get("CI"),
                         "native macOS external-storage bind check")
    def test_native_default_runtime_binds_private_socket_fixtures(self):
        # Never silently fall back to internal storage when the NVMe is absent.
        self.assertTrue(NVME.is_mount(), "the required external volume is not mounted")
        code = '''
import json, os, pathlib, socket, stat, tempfile
root = pathlib.Path(os.environ["SYMAIRA_EXTERNAL_RUNTIME_ROOT"])
assert root.resolve().is_relative_to(pathlib.Path("/Volumes/1TB_NVMe_SN850X"))
for name in ("TMPDIR", "TMP", "TEMP"):
    assert pathlib.Path(os.environ[name]) == root
paths = []
for prefix, suffix in (("sb-autostart-ok-", "autostart-ok.sock"),
                       ("sb-client-test-", "mock.sock"),
                       ("sb-race-", "race.sock"), ("sb-stale-", "stale.sock"),
                       ("sb-budget-", "default.sock"), ("mcp-", "test.sock"),
                       ("sb-", "Library/Caches/symbrowse/run/c1234.sock")):
    with tempfile.TemporaryDirectory(prefix=prefix) as directory:
        assert pathlib.Path(directory).parent == root
        assert stat.S_IMODE(os.stat(directory).st_mode) == 0o700
        path = pathlib.Path(directory) / suffix
        path.parent.mkdir(parents=True, exist_ok=True)
        assert len(os.fsencode(path)) < 104, str(path)
        with socket.socket(socket.AF_UNIX) as listener:
            listener.bind(str(path))
        paths.append({"prefix": prefix, "socket_bytes": len(os.fsencode(path))})
    assert not pathlib.Path(directory).exists()
print(json.dumps(paths))
'''
        result = subprocess.run(
            ["bash", str(WRAPPER), sys.executable, "-c", code],
            env=clean_environment(), capture_output=True, text=True, timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(json.loads(result.stdout)), 7)


if __name__ == "__main__":
    unittest.main()
