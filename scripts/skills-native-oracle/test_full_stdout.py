"""One native check of the actual full-stdout prerequisite and cleanup."""
import errno
import json
import os
from pathlib import Path
import tempfile
import unittest

from full_stdout import full_stdout


class FullStdout(unittest.TestCase):
    @unittest.skipIf(os.name == "nt", "existing Windows matrix has no full-device case")
    def test_actual_enospc_without_changing_an_adjacent_host_file(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            sibling = root / "retained"
            sibling.write_bytes(b"unchanged\n")
            proof = {}
            with full_stdout(root / "full", proof) as stream:
                with self.assertRaises(OSError) as caught:
                    os.write(stream.fileno(), b"native stdout check\n")
                self.assertEqual(caught.exception.errno, errno.ENOSPC)
                self.assertEqual(proof["actual_errno"], errno.ENOSPC)
                self.assertEqual(proof["status"], "passed")
            self.assertEqual(sibling.read_bytes(), b"unchanged\n")
            if proof["backend"] == "owned-HFS+-image":
                self.assertLessEqual(proof["image_bytes"], 16 * 1024 * 1024)
                self.assertEqual(proof["cleanup"], "no-owned-image-attached")
            print(json.dumps(proof))


if __name__ == "__main__":
    unittest.main()
