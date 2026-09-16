"""Mutation controls for the source/toolchain-bound raw-byte fixture."""
import copy
import json
from pathlib import Path
import tempfile
import unittest

from trust_anchor import file_sha256, load_anchor


HERE = Path(__file__).resolve().parent
FIXTURE = HERE / "../../../rust/symbrain-guard-core/tests/fixtures/external_repair_oracle.json"
TRUST_ANCHOR = load_anchor()
TRUSTED_ORACLE_COMMIT = TRUST_ANCHOR.oracle_commit
TRUSTED_GO_TOOLCHAIN = TRUST_ANCHOR.go_toolchain
TRUSTED_FIXTURE_SHA256 = TRUST_ANCHOR.fixture_sha256
TRUSTED_GENERATOR_SHA256 = TRUST_ANCHOR.generator_sha256
TRUSTED_VALIDATOR_SHA256 = TRUST_ANCHOR.validator_sha256
class RawByteFixtureTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture_path = FIXTURE.resolve()
        cls.original = json.loads(cls.fixture_path.read_bytes())
        cls.assert_trusted_identity(
            cls.fixture_path,
            HERE / "repair_parity.py",
            HERE / "native_repair.py",
            TRUSTED_ORACLE_COMMIT,
            TRUSTED_GO_TOOLCHAIN,
        )

    @staticmethod
    def assert_trusted_identity(
        fixture_path, generator_path, validator_path, oracle_commit, go_toolchain
    ):
        fixture_path = Path(fixture_path)
        generator_path = Path(generator_path)
        validator_path = Path(validator_path)
        if file_sha256(fixture_path) != TRUSTED_FIXTURE_SHA256:
            raise AssertionError("trusted fixture digest mismatch")
        if oracle_commit != TRUSTED_ORACLE_COMMIT:
            raise AssertionError("trusted source pin mismatch")
        if go_toolchain != TRUSTED_GO_TOOLCHAIN:
            raise AssertionError("trusted Go toolchain mismatch")
        if file_sha256(generator_path, normalize_crlf=True) != TRUSTED_GENERATOR_SHA256:
            raise AssertionError("trusted generator digest mismatch")
        if file_sha256(validator_path, normalize_crlf=True) != TRUSTED_VALIDATOR_SHA256:
            raise AssertionError("trusted validator digest mismatch")

    def assert_fixture_mutation_rejected(self, mutate):
        # First prove the unmodified fixture is accepted by independent anchors
        # rather than a baseline read from the candidate fixture.
        self.assert_trusted_identity(
            self.fixture_path,
            HERE / "repair_parity.py",
            HERE / "native_repair.py",
            TRUSTED_ORACLE_COMMIT,
            TRUSTED_GO_TOOLCHAIN,
        )
        altered = copy.deepcopy(self.original)
        mutate(altered)
        with tempfile.NamedTemporaryFile("wb", suffix=".json", delete=False) as handle:
            path = Path(handle.name)
            handle.write((json.dumps(altered, indent=2) + "\n").encode())
        try:
            with self.assertRaisesRegex(AssertionError, "trusted fixture digest mismatch"):
                self.assert_trusted_identity(
                    path,
                    HERE / "repair_parity.py",
                    HERE / "native_repair.py",
                    TRUSTED_ORACLE_COMMIT,
                    TRUSTED_GO_TOOLCHAIN,
                )
        finally:
            path.unlink()

    def test_current_fixture_is_accepted(self):
        self.assert_trusted_identity(
            self.fixture_path,
            HERE / "repair_parity.py",
            HERE / "native_repair.py",
            TRUSTED_ORACLE_COMMIT,
            TRUSTED_GO_TOOLCHAIN,
        )

    def test_generator_file_mutation_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            generator = Path(directory) / "repair_parity.py"
            generator.write_bytes((HERE / "repair_parity.py").read_bytes() + b"\n# disposable mutation\n")
            with self.assertRaisesRegex(AssertionError, "trusted generator digest mismatch"):
                self.assert_trusted_identity(
                    self.fixture_path,
                    generator,
                    HERE / "native_repair.py",
                    TRUSTED_ORACLE_COMMIT,
                    TRUSTED_GO_TOOLCHAIN,
                )

    def test_validator_file_mutation_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            validator = Path(directory) / "native_repair.py"
            validator.write_bytes((HERE / "native_repair.py").read_bytes() + b"\n# disposable mutation\n")
            with self.assertRaisesRegex(AssertionError, "trusted validator digest mismatch"):
                self.assert_trusted_identity(
                    self.fixture_path,
                    HERE / "repair_parity.py",
                    validator,
                    TRUSTED_ORACLE_COMMIT,
                    TRUSTED_GO_TOOLCHAIN,
                )

    def test_source_pin_metadata_mutation_is_rejected(self):
        with self.assertRaisesRegex(AssertionError, "trusted source pin mismatch"):
            self.assert_trusted_identity(
                self.fixture_path,
                HERE / "repair_parity.py",
                HERE / "native_repair.py",
                "1" * 40,
                TRUSTED_GO_TOOLCHAIN,
            )

    def test_toolchain_metadata_mutation_is_rejected(self):
        with self.assertRaisesRegex(AssertionError, "trusted Go toolchain mismatch"):
            self.assert_trusted_identity(
                self.fixture_path,
                HERE / "repair_parity.py",
                HERE / "native_repair.py",
                TRUSTED_ORACLE_COMMIT,
                "go1.26.6",
            )

    def test_expected_raw_response_mutation_is_rejected(self):
        self.assert_fixture_mutation_rejected(
            lambda fixture: fixture["cases"][0]["response"].__setitem__("reason", "mutated")
        )

    def test_source_pin_mutation_is_rejected(self):
        self.assert_fixture_mutation_rejected(
            lambda fixture: fixture.__setitem__("oracle_commit", "1" * 40)
        )

    def test_source_file_mutation_is_rejected(self):
        self.assert_fixture_mutation_rejected(
            lambda fixture: fixture["source_files"].__setitem__(
                next(iter(fixture["source_files"])), "0" * 64
            )
        )

    def test_toolchain_mutation_is_rejected(self):
        self.assert_fixture_mutation_rejected(
            lambda fixture: fixture.__setitem__("go_toolchain", "go1.26.6")
        )

    def test_generator_mutation_is_rejected(self):
        self.assert_fixture_mutation_rejected(
            lambda fixture: fixture.__setitem__("generator_sha256", "0" * 64)
        )

    def test_validation_basis_mutation_is_rejected(self):
        self.assert_fixture_mutation_rejected(
            lambda fixture: fixture["validation_basis"].__setitem__("compared", ["response"])
        )

    def test_validation_basis_hash_mutation_is_rejected(self):
        self.assert_fixture_mutation_rejected(
            lambda fixture: fixture.__setitem__("validation_basis_sha256", "0" * 64)
        )

    def test_case_inventory_mutation_is_rejected(self):
        self.assert_fixture_mutation_rejected(
            lambda fixture: fixture.__setitem__("case_count", fixture["case_count"] - 1)
        )

    def test_case_input_mutation_is_rejected(self):
        self.assert_fixture_mutation_rejected(
            lambda fixture: fixture["cases"][0].__setitem__("input_hex", "00")
        )

    def test_generator_has_no_mutable_root_identity(self):
        generator = (HERE / "repair_parity.py").read_text(encoding="utf-8")
        self.assertNotIn(TRUSTED_ORACLE_COMMIT, generator)
        self.assertNotIn(TRUSTED_GO_TOOLCHAIN, generator)

    def test_rust_anchor_carries_source_and_case_identity(self):
        self.assertEqual(len(TRUST_ANCHOR.source_files), 3)
        self.assertEqual(TRUST_ANCHOR.case_count, 152)

    def test_fixture_identity_does_not_use_binary_hash(self):
        self.assertNotIn("go_binary_sha256", self.original)


if __name__ == "__main__":
    unittest.main()
