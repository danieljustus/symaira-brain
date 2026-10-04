"""Pure source admission controls; no SDK, compiler or product invocation."""
import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("proof", HERE / "binding.py")
proof = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(proof)


class Admission(unittest.TestCase):
    def test_original_full60_and_three_controls_are_retained(self):
        self.assertEqual(len(proof.cases()), 60)
        self.assertEqual(proof.trusted()["controls"], ["changed-input", "zero-selected", "output-obstruction"])

    def test_omitted_reordered_and_changed_case_are_rejected(self):
        plan = proof.trusted()
        original = proof.cases()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "trusted.json").write_text(json.dumps(plan))
            for value in [original[:1], list(reversed(original)), original[:-1]]:
                (root / "cases.json").write_text(json.dumps(value))
                with patch.object(proof, "HERE", root), self.assertRaises(ValueError):
                    proof.cases()

    def test_caller_selected_oracle_identity_is_rejected(self):
        with patch.object(proof, "git", return_value="caller-selected-altered-go"), self.assertRaisesRegex(ValueError, "dcddcef0"):
            proof.sources(Path("unused"), proof.ROOT)

    def test_caller_selected_oracle_hash_map_is_rejected(self):
        with patch.object(proof, "git", side_effect=[proof.GO_REF, ""]), patch.object(proof, "source_map", return_value={"altered.go": "invented"}), self.assertRaisesRegex(ValueError, "independent frozen map"):
            proof.sources(Path("unused"), proof.ROOT)

    def test_wrong_sdk_bytes_are_rejected(self):
        with patch.object(proof, "digest", return_value="invented"), self.assertRaisesRegex(ValueError, "unbound SDK bytes"):
            proof.sdks(Path("owned-go"), Path("owned-rust"))

    def test_missing_platform_cannot_become_native_acceptance(self):
        with patch.object(proof.platform, "system", return_value="Unallocated"), self.assertRaisesRegex(ValueError, "not yet independently pinned"):
            proof.sdks(Path("unused"), Path("unused"))

    def test_changed_executable_cannot_reuse_actual_build_receipt(self):
        before = {"binaries": {"go": "original", "rust": "original"}}
        receipt = {"kind": "actual-memory760-build-v1", "snapshot": copy.deepcopy(before)}
        before["binaries"]["go"] = "changed-after-process"
        with self.assertRaisesRegex(ValueError, "executable build binding"):
            proof.verify_receipt(receipt, before, Path("unused"))

    def test_arbitrary_sdk_labels_and_empty_builds_are_rejected(self):
        with self.assertRaises(ValueError):
            proof.verify_receipt({"go_sdk": "not-an-actual-SDK-receipt"}, {}, Path("unused"))
        with self.assertRaisesRegex(ValueError, "both actual builds"):
            proof.verify_receipt({"kind": "actual-memory760-build-v1", "snapshot": {}, "steps": []}, {}, Path("unused"))


if __name__ == "__main__":
    unittest.main()
