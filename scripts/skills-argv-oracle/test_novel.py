"""Portable preparation tests; these do not execute a Go or Rust CLI."""
import json
import os
from pathlib import Path
import tempfile
import unittest

from novel import PLAN, PLAN_SHA, arguments, digest, seed, selected_cases, snapshot


class NovelPreparation(unittest.TestCase):
    def test_original_plan_and_native_scope(self):
        self.assertEqual(digest(PLAN.read_bytes()), PLAN_SHA)
        self.assertEqual(len(json.loads(PLAN.read_bytes())["novel_cases"]), 152)
        for windows, required, inherited in [(False, 51, 2), (True, 109, 3)]:
            cases = selected_cases(windows)
            self.assertEqual(len(cases), required + inherited)
            self.assertEqual(sum(case["id"].startswith("inherited-global") for case in cases), inherited)
            self.assertEqual(len({case["id"] for case in cases}), len(cases))

    def test_windows_code_units_survive_every_selected_vector(self):
        saw_lone_surrogate = False
        for case in selected_cases(True):
            values = arguments(case, True)
            expected = case.get("args_utf16le_hex")
            if expected is None:
                expected = [value.encode("utf-16-le").hex() for value in case["args"]]
            self.assertEqual([value.encode("utf-16-le", "surrogatepass").hex() for value in values], expected)
            saw_lone_surrogate |= any(any(0xD800 <= ord(char) <= 0xDFFF for char in value) for value in values)
        self.assertTrue(saw_lone_surrogate)

    def test_unix_bytes_survive_and_are_rejected_as_windows_vectors(self):
        saw_invalid_utf8 = False
        for case in selected_cases(False):
            values = arguments(case, False)
            if "args" in case:
                self.assertEqual(values, [value.encode("utf-8") for value in case["args"]])
            if "args_bytes_hex" in case:
                self.assertEqual([value.hex() for value in values], case["args_bytes_hex"])
                with self.assertRaises(ValueError):
                    arguments(case, True)
                for value in values:
                    try:
                        value.decode("utf-8")
                    except UnicodeDecodeError:
                        saw_invalid_utf8 = True
        self.assertTrue(saw_invalid_utf8)

    def test_seeded_full_state_detects_content_metadata_and_links(self):
        with tempfile.TemporaryDirectory(prefix="skills-novel-test-") as temporary:
            root = Path(temporary)
            seed(root)
            initial = snapshot(root)
            self.assertEqual(initial, snapshot(root))
            rows = {bytes.fromhex(row["relative_path_bytes_hex"]): row for row in initial}
            owned = root / "project/owned.txt"
            row = rows[os.fsencode(Path("project/owned.txt"))]
            self.assertEqual(bytes.fromhex(row["bytes_hex"]), b"unchanged\0raw\xff")
            self.assertEqual(row["mtime_ns"], 10**18)
            owned.write_bytes(b"changed\0raw\xff")
            os.utime(owned, ns=(10**18, 10**18))
            self.assertNotEqual(initial, snapshot(root))
            owned.write_bytes(b"unchanged\0raw\xff")
            # A one-second delta is representable by native Windows FILETIME.
            os.utime(owned, ns=(10**18 + 10**9, 10**18 + 10**9))
            self.assertNotEqual(initial, snapshot(root))
            if os.name != "nt":
                owned.chmod(0o600)
                os.utime(owned, ns=(10**18, 10**18))
                self.assertNotEqual(initial, snapshot(root))
                link = root / "owned-link"
                link.symlink_to("project/owned.txt")
                links = [row for row in snapshot(root) if "link_target_bytes_hex" in row]
                self.assertEqual(bytes.fromhex(links[0]["link_target_bytes_hex"]), b"project/owned.txt")


if __name__ == "__main__":
    unittest.main()
