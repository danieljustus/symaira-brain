"""Bounded filesystem admission proofs, not SDK/compiler/product execution."""
import errno
import hashlib
import json
import multiprocessing
import os
from pathlib import Path
import stat
import tempfile
import unittest
from unittest.mock import patch
import binding
import sdk
import sdk_fixture


ORIGINAL_CASES = ("clean", "fifo:go:bin/go", "fifo:go:src/fmt/doc.go",
                  "fifo:rust:bin/rustc", "fifo:go:src/fmt/extra.pipe",
                  "config-link", "directory-mode", "header-removal",
                  "missing-directory", "root-link")


def child_admission(value, send, legacy_first):
    try:
        with sdk_fixture.selected(binding, value):
            if legacy_first:
                # Deliberate old ordering mutant: the unchanged gate must
                # detect a blocking read of each originally mapped FIFO.
                for role in ("go", "rust"):
                    plan = value["plan"]["sdk_platforms"]["linux-x86_64"][role]
                    for name in {**plan["files"], **plan.get("source_files", {})}:
                        binding.digest(value[role] / name)
            with patch.object(sdk, "sha", wraps=sdk.sha) as reads:
                binding.sdks(value["go"], value["rust"])
            result = {"status": "accepted", "sdk_hash_reads": reads.call_count}
    except Exception as error:
        result = {"status": "rejected", "error": type(error).__name__,
                  "detail": str(error), "sdk_hash_reads": reads.call_count if "reads" in locals() else 0}
    send.send(result)
    send.close()


def observe(case, legacy_first=False):
    with tempfile.TemporaryDirectory() as temporary:
        value = sdk_fixture.prepare(Path(temporary))
        selected = None
        if case.startswith("fifo:"):
            _, role, name = case.split(":", 2)
            selected = value[role] / name
            if selected.exists():
                selected.unlink()
            os.mkfifo(selected)
        elif case == "config-link":
            selected = value["go"] / "go.env"
            selected.unlink()
            selected.symlink_to("src/fmt/doc.go")
        elif case == "directory-mode":
            (value["rust"] / "lib").chmod(0o775)
        elif case == "header-removal":
            (value["go"] / "pkg/include/textflag.h").unlink()
        elif case == "missing-directory":
            # Preserve the original review identity: this case adds an
            # empty directory; it is not reinterpreted as a removal.
            (value["go"] / "empty").mkdir()
        elif case == "root-link":
            alias = Path(temporary) / "alias"
            alias.symlink_to(value["go"], target_is_directory=True)
            value["go"] = alias
        receive, send = multiprocessing.Pipe(duplex=False)
        child = multiprocessing.get_context("fork").Process(
            target=child_admission, args=(value, send, legacy_first))
        child.start()
        send.close()
        try:
            child.join(0.5)
            blocked = child.is_alive()
            if blocked:
                child.terminate()
                child.join(2)
            if child.is_alive():
                child.kill()
                child.join(2)
                raise AssertionError("owned admission child did not terminate")
            result = {"status": "blocked-before-refusal"} if blocked else receive.recv()
            row = {"case": case, "legacy_first_mutant": legacy_first,
                   "waited_seconds": 0.5, "timed_out": blocked, "child_exit": child.exitcode,
                   "expected_inventory": value["expected"], "legacy_plan": value["plan"],
                   "regular_input_hex": b"owned source bytes\n".hex(),
                   "selected_lstat_mode": selected.lstat().st_mode if selected else None,
                   "SDK_compiler_product_runs": 0, **result}
            if os.environ.get("MEMORY760_ADMISSION_JOURNAL"):
                with Path(os.environ["MEMORY760_ADMISSION_JOURNAL"]).open("a") as journal:
                    journal.write(json.dumps(row) + "\n")
            return row
        finally:
            if child.is_alive():
                child.kill()
                child.join(2)
            receive.close()
            child.close()


class ReadAdmission(unittest.TestCase):
    @unittest.skipUnless(hasattr(os, "mkfifo") and "fork" in multiprocessing.get_all_start_methods(),
                         "requires POSIX owned FIFO/process controls")
    def test_original_ten_composite_conditions_are_bounded(self):
        for case in ORIGINAL_CASES:
            with self.subTest(case=case):
                row = observe(case)
                self.assertFalse(row["timed_out"], row)
                self.assertEqual(row["child_exit"], 0, row)
                self.assertEqual(row["status"], "accepted" if case == "clean" else "rejected", row)
                self.assertEqual(row["sdk_hash_reads"], 7 if case == "clean" else 0, row)

    @unittest.skipUnless(hasattr(os, "mkfifo") and "fork" in multiprocessing.get_all_start_methods(),
                         "requires POSIX owned FIFO/process controls")
    def test_all_three_original_legacy_read_mutants_are_detected(self):
        for case in ORIGINAL_CASES[1:4]:
            with self.subTest(case=case):
                row = observe(case, legacy_first=True)
                self.assertTrue(row["timed_out"], row)
                self.assertEqual(row["status"], "blocked-before-refusal", row)
                self.assertEqual(row["child_exit"], -15, row)

    def test_both_whole_sdk_names_are_admitted_before_either_hash(self):
        with tempfile.TemporaryDirectory() as temporary:
            value = sdk_fixture.prepare(Path(temporary))
            (value["rust"] / "added-input").write_bytes(b"foreign")
            with sdk_fixture.selected(binding, value), patch.object(sdk, "sha") as reads:
                with self.assertRaisesRegex(ValueError, "inventory: rust:files:extra"):
                    binding.sdks(value["go"], value["rust"])
                reads.assert_not_called()

    def test_legacy_hashes_remain_mandatory_after_whole_admission(self):
        with tempfile.TemporaryDirectory() as temporary:
            value = sdk_fixture.prepare(Path(temporary))
            value["plan"]["sdk_platforms"]["linux-x86_64"]["go"]["source_files"]["src/fmt/doc.go"] = "changed"
            with sdk_fixture.selected(binding, value), patch.object(binding, "digest") as old_read:
                with self.assertRaisesRegex(ValueError, "unbound SDK bytes: go:src/fmt/doc.go"):
                    binding.sdks(value["go"], value["rust"])
                old_read.assert_not_called()

    def test_checked_same_descriptor_hash_and_close(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "input"
            data = b"owned\x00input\xff"
            path.write_bytes(data)
            close = os.close
            with patch.object(sdk.os, "close", wraps=close) as closed:
                self.assertEqual(sdk.sha(path), hashlib.sha256(data).hexdigest())
                descriptor = closed.call_args.args[0]
                with self.assertRaises(OSError) as error:
                    os.fstat(descriptor)
                self.assertEqual(error.exception.errno, errno.EBADF)

    def test_stale_mode_refused_before_open(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "input"
            path.write_bytes(b"owned")
            mode = stat.S_IMODE(path.stat().st_mode)
            path.chmod(mode ^ 0o020)
            with patch.object(sdk.os, "open") as opened:
                with self.assertRaisesRegex(ValueError, "bytes/mode changed"):
                    sdk.sha(path, expected_mode=mode)
                opened.assert_not_called()

    @unittest.skipUnless(hasattr(os, "O_NONBLOCK") and hasattr(os, "O_NOFOLLOW") and hasattr(os, "mkfifo"),
                         "requires POSIX no-follow/nonblocking descriptor admission")
    def test_fifo_substituted_between_lstat_and_open_is_closed_before_read(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "input"
            path.write_bytes(b"owned")
            actual_open, actual_close = os.open, os.close
            def swapped(candidate, flags):
                path.unlink()
                os.mkfifo(path)
                self.assertTrue(flags & os.O_NONBLOCK)
                self.assertTrue(flags & os.O_NOFOLLOW)
                return actual_open(candidate, flags)
            with patch.object(sdk.os, "open", side_effect=swapped), patch.object(sdk.os, "read", side_effect=AssertionError("unsafe descriptor read")) as reads, patch.object(sdk.os, "close", wraps=actual_close) as closed:
                with self.assertRaisesRegex(ValueError, "identity/type changed"):
                    sdk.sha(path)
                reads.assert_not_called()
                closed.assert_called_once()
                with self.assertRaises(OSError):
                    os.fstat(closed.call_args.args[0])

    @unittest.skipUnless(hasattr(os, "O_NOFOLLOW"), "requires POSIX no-follow open")
    def test_leaf_alias_substituted_after_lstat_is_not_opened(self):
        with tempfile.TemporaryDirectory() as temporary:
            path, target = Path(temporary) / "input", Path(temporary) / "target"
            path.write_bytes(b"owned")
            target.write_bytes(b"foreign")
            actual_open = os.open
            def swapped(candidate, flags):
                path.unlink()
                path.symlink_to(target)
                return actual_open(candidate, flags)
            with patch.object(sdk.os, "open", side_effect=swapped), patch.object(sdk.os, "read", side_effect=AssertionError("unsafe descriptor read")) as reads:
                with self.assertRaises(OSError) as error:
                    sdk.sha(path)
                self.assertEqual(error.exception.errno, errno.ELOOP)
                reads.assert_not_called()

    def test_regular_substitution_identity_is_refused_before_read(self):
        with tempfile.TemporaryDirectory() as temporary:
            path, replacement = Path(temporary) / "input", Path(temporary) / "replacement"
            path.write_bytes(b"owned")
            replacement.write_bytes(b"different regular input")
            actual_open = os.open
            def swapped(candidate, flags):
                replacement.replace(path)
                return actual_open(candidate, flags)
            with patch.object(sdk.os, "open", side_effect=swapped), patch.object(sdk.os, "read", side_effect=AssertionError("unsafe descriptor read")) as reads:
                with self.assertRaisesRegex(ValueError, "identity/type changed"):
                    sdk.sha(path)
                reads.assert_not_called()


if __name__ == "__main__":
    unittest.main()
