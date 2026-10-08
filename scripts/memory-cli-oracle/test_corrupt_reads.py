"""Unit controls for diagnostic cause checks, not extra process observations."""
import base64
from pathlib import Path
import unittest

from corrupt_reads import diagnostic_contract


def observation(exit_code, stderr, stdout=b""):
    return {"exit": exit_code, "stderr": base64.b64encode(stderr).decode(),
            "stdout": base64.b64encode(stdout).decode()}


class DiagnosticContract(unittest.TestCase):
    def test_exact_decoder_and_owned_missing_binary_causes_are_required(self):
        root = Path("owned-case")
        decoder = observation(1, b"symbrain memory search: search memories: "
                              b"invalid character 'b' looking for beginning of value\n")
        absent = observation(1, f"symbrain: start Go fallback {root / 'absent-go'}: missing (os error 2)\n".encode())
        check = lambda go, native: diagnostic_contract(go, native, "search", "metadata", "broken", root)
        self.assertEqual(check(decoder, absent), (True, True))
        generic = observation(1, b"symbrain memory search: search memories: transport unavailable\n")
        self.assertEqual(check(generic, absent), (False, True))
        for wrong in (
                observation(1, b"some other native failure\n"),
                observation(1, f"symbrain: start Go fallback {root / 'wrong-go'}: missing (os error 2)\n".encode()),
                observation(1, f"symbrain: start Go fallback {root / 'absent-go'}: denied (os error 13)\n".encode()),
                observation(2, base64.b64decode(absent["stderr"])),
                observation(1, base64.b64decode(absent["stderr"]), b"partial success")):
            with self.subTest(wrong=wrong):
                self.assertEqual(check(decoder, wrong), (True, False))
        positive = observation(0, b"", b"[]\n")
        self.assertEqual(diagnostic_contract(positive, positive, "list", "metadata", '{"n":"value"}', root),
                         (True, True))


if __name__ == "__main__":
    unittest.main()
