#!/usr/bin/env python3
"""Every report run.sh writes must be uploaded by every native Guard job (#941).

The upload lists in ci.yml are maintained by hand, so a new report can be
produced and executed on every target yet silently dropped from the retained
artifact. This derives the expected paths from run.sh itself.
"""
from pathlib import Path
import re
import unittest

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
STEP = "- name: Upload native standalone Guard evidence"
TARGETS = 2  # Linux and macOS arm64; Windows acceptance was retired in #990.


def expected_paths(run_sh):
    names = {"guard-standalone.json", "guard-standalone-binaries/"}
    for suffix in re.findall(r'"\$\{report%\.json\}-([A-Za-z0-9_.-]+)"', run_sh):
        names.add("guard-standalone-" + suffix)
    return names


def upload_blocks(workflow):
    blocks = []
    for chunk in workflow.split(STEP)[1:]:
        body = chunk.split("\n      - name:", 1)[0]
        listed = re.findall(r"\$\{\{ runner\.temp \}\}/(\S+)", body)
        blocks.append(set(listed))
    return blocks


def missing(run_sh, workflow):
    expected = expected_paths(run_sh)
    return [sorted(expected - block) for block in upload_blocks(workflow)]


class GuardUploadInventoryTests(unittest.TestCase):
    def setUp(self):
        self.run_sh = (HERE / "run.sh").read_text()
        self.workflow = (ROOT / ".github/workflows/ci.yml").read_text()

    def test_every_native_target_uploads_every_report(self):
        blocks = upload_blocks(self.workflow)
        self.assertEqual(len(blocks), TARGETS)
        self.assertIn("guard-standalone-scan-boundaries.json", expected_paths(self.run_sh))
        self.assertEqual(missing(self.run_sh, self.workflow), [[]] * TARGETS)

    def test_omitted_report_is_rejected(self):
        line = "            ${{ runner.temp }}/guard-standalone-scan-boundaries.json\n"
        self.assertEqual(self.workflow.count(line), TARGETS)
        for index in range(TARGETS):
            with self.subTest(target=index):
                parts = self.workflow.split(line)
                mutated = "".join(
                    part + ("" if i == index else line) for i, part in enumerate(parts[:-1])
                ) + parts[-1]
                result = missing(self.run_sh, mutated)
                self.assertEqual(result[index], ["guard-standalone-scan-boundaries.json"])
                self.assertEqual(sum(bool(r) for r in result), 1)


if __name__ == "__main__":
    unittest.main()
