import json
import unittest

from chrome_pair import (
    FIXTURE_TITLE, FIXTURE_TOKEN, native_target_matches, nearest_rank,
    paired_gate_passes, validate_read_output,
)


class ChromePairTests(unittest.TestCase):
    def test_read_requires_fixture_title_and_content_token(self):
        good = json.dumps({"success": True, "data": {"title": FIXTURE_TITLE, "markdown": FIXTURE_TOKEN}})
        self.assertTrue(validate_read_output(good))
        self.assertFalse(validate_read_output(json.dumps({"success": True, "data": {"title": FIXTURE_TITLE}})))
        self.assertFalse(validate_read_output("not json"))

    def test_nearest_rank_matches_contract(self):
        self.assertEqual(nearest_rank(list(range(1, 31))), 29)

    def test_paired_gate_needs_complete_samples_and_limits_regression(self):
        go = list(range(100, 130))
        self.assertTrue(paired_gate_passes(go, go, 30))
        self.assertFalse(paired_gate_passes(go, [value * 2 for value in go], 30))
        self.assertFalse(paired_gate_passes(go[:-1], go, 30))

    def test_native_runner_architecture_is_exact_and_win_arm_is_known(self):
        self.assertTrue(native_target_matches("linux-arm64", "Linux", "aarch64"))
        self.assertFalse(native_target_matches("linux-arm64", "Linux", "x86_64"))
        self.assertTrue(native_target_matches("windows-arm64", "Windows", "ARM64"))


if __name__ == "__main__":
    unittest.main()
