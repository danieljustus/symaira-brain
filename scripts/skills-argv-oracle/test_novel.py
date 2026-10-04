"""Portable preparation tests; these do not execute a Go or Rust CLI."""
import json
import os
import subprocess
from pathlib import Path
import tempfile
import unittest

from inherited_output import CLI, MAIN, OWNER_PATHS, PARENT_REFS, early_blocks, qualify
from novel import PLAN, PLAN_SHA, ROOT, arguments, digest, seed, selected_cases, snapshot


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


def sources(reference):
    return {
        name: subprocess.check_output(["git", "show", f"{reference}:{name}"], cwd=ROOT)
        for name in OWNER_PATHS
    }


class InheritedOutputSource(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.parent = sources(PARENT_REFS[0])
        cls.current = {name: (ROOT / name).read_bytes() for name in OWNER_PATHS}

    def test_actual_windows_parent_owner_and_allowed_reference_names(self):
        # Unix runtime must still bind its actual parent independently. This
        # source test needs only the Windows pin explicitly fetched by CI.
        for reference in PARENT_REFS:
            self.assertEqual(qualify(reference, self.parent, self.parent)["qualification"], "original-parent")

    def test_reviewed_complete_profiles_bind_real_git_bodies(self):
        parent = self.parent
        for reference, expected in [
            ("5a4f0464bc24efefa82e26068276fc747c5b1b11", "memory803-5a4"),
            ("cdb2aca223bd9030f834608fd13cc4e643ebdc68", "guard805-cdb"),
        ]:
            with self.subTest(reference=reference):
                self.assertEqual(qualify(PARENT_REFS[0], parent, sources(reference))["qualification"], expected)
        result = qualify(PARENT_REFS[0], parent, self.current)
        self.assertEqual(result["qualification"], "memory803-reviewed-guard-integration")
        self.assertFalse(result["go_parity_claim"])
        self.assertEqual(set(result["current_owner_sources_sha256"]), set(OWNER_PATHS))

    def test_early_return_is_byte_exact_before_command_handlers(self):
        parent = self.parent
        old = early_blocks(parent[CLI], False)
        new = early_blocks(self.current[CLI], True)
        self.assertEqual(old, new)
        self.assertIn(b'return Some(exit::USAGE);', new["early_output_return"])
        self.assertIn(b'writeln!(stderr, "symbrain: {err}")', new["early_output_return"])
        self.assertNotIn(b"normalize_flags", new["early_output_return"])

    def test_each_changed_current_owner_requires_fresh_review(self):
        parent = self.parent
        for name in OWNER_PATHS:
            changed = {**self.current, name: self.current[name] + b"\n// unreviewed\n"}
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, "explicit source review"):
                qualify(PARENT_REFS[0], parent, changed)

    def test_each_changed_parent_owner_is_refused(self):
        parent = self.parent
        for name in OWNER_PATHS:
            changed = {**parent, name: parent[name] + b"\n// not the pinned parent\n"}
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, "immutable reviewed source"):
                qualify(PARENT_REFS[0], changed, self.current)

    def test_unknown_reference_is_not_a_synthetic_source_binding(self):
        with self.assertRaisesRegex(ValueError, "immutable reviewed source"):
            qualify("0" * 40, self.parent, self.current)

    def test_entry_point_cannot_be_mixed_with_another_library(self):
        parent = self.parent
        changed = {**self.current, MAIN: parent[MAIN]}
        with self.assertRaisesRegex(ValueError, "explicit source review"):
            qualify(PARENT_REFS[0], parent, changed)

    def test_missing_or_extra_owner_cannot_skip_a_dependency(self):
        parent = self.parent
        for changed in [{name: data for name, data in self.current.items() if name != MAIN},
                        {**self.current, "extra": b"unrelated"}]:
            with self.assertRaisesRegex(ValueError, "source set differs"):
                qualify(PARENT_REFS[0], parent, changed)


if __name__ == "__main__":
    unittest.main()
