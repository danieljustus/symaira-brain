"""Executable orchestration controls; these do not certify Apple signing."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("notarize-and-verify.sh")
SUBMISSION = "a14be539-e270-44bd-9db1-52e2875446da"
FAKE = r'''#!/usr/bin/env python3
import json, os, pathlib, sys
name=pathlib.Path(sys.argv[0]).name
args=sys.argv[1:]
if name=='xcrun': stage='-'.join(args[:2])
elif name=='spctl': stage='assessment'
elif name=='codesign': stage='signature'
else: stage=name
with open(os.environ['TRACE'],'a') as log: log.write(json.dumps([stage,args])+'\n')
if stage==os.environ.get('FAIL_STAGE'): sys.exit(23)
if stage=='signature':
    expected='certificate leaf[subject.OU] = "M4744F3TAA"'
    if '-R' in args and expected not in args[args.index('-R')+1]: sys.exit(24)
    if '-R' not in args and '--deep' not in args: sys.exit(24)
if stage in ('notarytool-submit','notarytool-info'):
    identifier='a14be539-e270-44bd-9db1-52e2875446da'
    if stage=='notarytool-info' and os.environ.get('WRONG_ID'):
        identifier='d16003f8-dd27-46d5-b7e7-9684dd6f2074'
    print(json.dumps({'id':identifier,'status':os.environ.get('NOTARY_STATUS','Accepted')}))
elif stage=='ditto': pathlib.Path(args[-1]).write_bytes(b'owned fake submission')
'''


class ReleaseVerification(unittest.TestCase):
    def replay(self, kind="app", verify_only=False, overrides=None):
        with tempfile.TemporaryDirectory(prefix="notary contract ") as directory:
            root = Path(directory)
            binary = root / "bin"
            binary.mkdir()
            for name in ("codesign", "xcrun", "spctl", "ditto", "publish"):
                path = binary / name
                path.write_text(FAKE)
                path.chmod(0o700)
            artifact = root / ("Symaira Brain.app" if kind == "app" else "release.dmg")
            if kind == "app":
                artifact.mkdir()
            else:
                artifact.write_bytes(b"owned artifact")
            trace = root / "trace"
            env = dict(os.environ, PATH=str(binary) + os.pathsep + os.environ["PATH"],
                       TRACE=str(trace), TEAM_ID="M4744F3TAA", NOTARY_API_KEY_ID="synthetic",
                       NOTARY_API_ISSUER_ID="synthetic", API_KEY_PATH=str(root / "synthetic.p8"))
            env.update(overrides or {})
            args = ["--verify-only"] if verify_only else []
            args += [str(artifact), kind, str(root / "notary")]
            result = subprocess.run(
                ["bash", "-c", 'set -euo pipefail; bash "$1" "${@:2:4}"; publish',
                 "--", str(SCRIPT), *args], env=env, capture_output=True, timeout=10)
            records = [json.loads(line) for line in trace.read_text().splitlines()] if trace.exists() else []
            receipts = {p.name: p.read_text() for p in root.glob("notary-*.json")}
            return result, records, receipts

    def test_app_and_dmg_require_verified_submission_and_readback_before_publication(self):
        for kind in ("app", "dmg"):
            with self.subTest(kind=kind):
                result, records, receipts = self.replay(kind)
                self.assertEqual(result.returncode, 0, result.stderr)
                stages = [stage for stage, _ in records]
                self.assertEqual(stages[-1], "publish")
                for stage in ("notarytool-submit", "notarytool-info", "stapler-staple", "stapler-validate", "assessment"):
                    self.assertIn(stage, stages)
                self.assertEqual(json.loads(receipts["notary-info.json"])["id"], SUBMISSION)
                self.assertEqual(sum(stage == "signature" for stage in stages), 4)
                assessment = next(args for stage, args in records if stage == "assessment")
                self.assertIn("execute" if kind == "app" else "open", assessment)
                if kind == "app":
                    self.assertTrue(any('identifier "com.symaira.brain"' in arg for _, args in records for arg in args))

    def test_each_real_command_failure_prevents_publication(self):
        for stage in ("signature", "ditto", "notarytool-submit", "notarytool-info", "stapler-staple", "stapler-validate", "assessment"):
            with self.subTest(stage=stage):
                result, records, _ = self.replay(overrides={"FAIL_STAGE": stage})
                self.assertEqual(result.returncode, 23)
                self.assertNotIn("publish", [x[0] for x in records])

    def test_nonaccepted_or_mismatched_readback_prevents_stapling_and_publication(self):
        for override in ({"NOTARY_STATUS": "Invalid"}, {"NOTARY_STATUS": "In Progress"}, {"WRONG_ID": "1"}):
            with self.subTest(override=override):
                result, records, _ = self.replay(overrides=override)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse({"publish", "stapler-staple"} & {x[0] for x in records})

    def test_downloaded_asset_requires_checks_without_resubmitting(self):
        result, records, receipts = self.replay(kind="dmg", verify_only=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([x[0] for x in records], ["signature", "signature", "stapler-validate", "signature", "signature", "assessment", "publish"])
        self.assertEqual(receipts, {})

    def test_invalid_team_prevents_any_external_execution(self):
        result, records, _ = self.replay(overrides={"TEAM_ID": 'wrong"team'})
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(records, [])


if __name__ == "__main__":
    unittest.main()
