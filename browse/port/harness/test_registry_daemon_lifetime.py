"""Pure ownership/API mocks; no SDK, product, native Windows or ports run."""
import subprocess
import unittest
from pathlib import Path
from unittest.mock import patch

import registry_daemon_lifetime as lifetime


class API:
    def __init__(self, waits, image="/owned/symbrowse.exe", exit_code=0):
        self.waits, self.actual_image, self.code = list(waits), image, exit_code
        self.handle = 0x100000005
        self.calls = []

    def open(self, pid):
        self.calls.append(("open", pid))
        return self.handle

    def image(self, handle):
        self.calls.append(("image", handle))
        return self.actual_image

    def wait(self, handle, seconds):
        self.calls.append(("wait", handle, seconds))
        value = self.waits.pop(0)
        if isinstance(value, BaseException):
            raise value
        return value

    def exit_code(self, handle):
        self.calls.append(("exit", handle))
        return self.code

    def terminate(self, handle):
        self.calls.append(("terminate", handle))

    def close(self, handle):
        self.calls.append(("close", handle))


class Tests(unittest.TestCase):
    def owner(self, api):
        return lifetime.WindowsOwner(123, Path("/owned/symbrowse.exe"), api=api)

    def test_normal_shutdown_waits_exit_then_closes_same_64bit_handle(self):
        api = API([False, True])
        owner = self.owner(api)
        owner.confirm(123, 123)
        owner.finish()
        self.assertEqual(api.calls, [("open", 123), ("image", api.handle),
            ("wait", api.handle, 0), ("wait", api.handle, 15),
            ("exit", api.handle), ("close", api.handle)])
        self.assertIsNone(owner.handle)

    def test_close_failure_retains_same_handle_for_retry(self):
        api = API([False, True])
        owner = self.owner(api)
        owner.confirm(123, 123)
        error = OSError("native CloseHandle failed")
        with patch.object(api, "close", side_effect=[error, None]) as close:
            with self.assertRaises(OSError) as failure:
                owner.finish()
            self.assertIs(failure.exception, error)
            self.assertEqual(owner.handle, api.handle)
            owner.close()
            self.assertIsNone(owner.handle)
            owner.close()
            self.assertEqual([call.args for call in close.call_args_list], [(api.handle,), (api.handle,)])
        self.assertFalse(any(call[0] == "terminate" for call in api.calls))

    def test_timeout_terminates_retained_object_but_still_fails_gate(self):
        api = API([False, False, False, True])
        owner = self.owner(api)
        owner.confirm(123, 123)
        with self.assertRaises(subprocess.TimeoutExpired) as failure:
            owner.finish()
        self.assertEqual(failure.exception.timeout, 15)
        self.assertEqual(api.calls[-3:], [("terminate", api.handle),
            ("wait", api.handle, 2), ("close", api.handle)])

    def test_stop_error_survives_cleanup_instead_of_becoming_success(self):
        api = API([False, False, True])
        owner = self.owner(api)
        owner.confirm(123, 123)
        original = OSError("owned stop transport failed")
        with self.assertRaises(OSError) as failure:
            owner.finish(original)
        self.assertIs(failure.exception, original)
        self.assertIn(("terminate", api.handle), api.calls)
        self.assertEqual(api.calls[-1], ("close", api.handle))

    def test_failed_forced_cleanup_remains_failure_with_timeout_cause(self):
        api = API([False, False, False, False])
        owner = self.owner(api)
        owner.confirm(123, 123)
        with self.assertRaisesRegex(AssertionError, "survived forced cleanup") as failure:
            owner.finish()
        self.assertIsInstance(failure.exception.__cause__, subprocess.TimeoutExpired)
        self.assertEqual(api.calls[-1], ("close", api.handle))

    def test_wrong_image_is_closed_without_termination(self):
        api = API([], image="/unrelated/symbrowse.exe")
        with self.assertRaisesRegex(AssertionError, "image differs"):
            self.owner(api)
        self.assertEqual(api.calls, [("open", 123), ("image", api.handle), ("close", api.handle)])

    def test_unconfirmed_identity_never_forces_process_cleanup(self):
        api = API([])
        owner = self.owner(api)
        with self.assertRaisesRegex(AssertionError, "unconfirmed"):
            owner.finish()
        self.assertEqual(api.calls[-1], ("close", api.handle))
        self.assertFalse(any(call[0] == "terminate" for call in api.calls))

    def test_changed_reported_owner_is_not_confirmed(self):
        api = API([])
        owner = self.owner(api)
        with self.assertRaisesRegex(AssertionError, "owner changed"):
            owner.confirm(123, 124)
        self.assertFalse(owner.confirmed)
        owner.close()
        self.assertFalse(any(call[0] == "terminate" for call in api.calls))

    def test_already_exited_owner_before_confirmation_is_refused(self):
        api = API([True])
        owner = self.owner(api)
        with self.assertRaisesRegex(AssertionError, "owner changed"):
            owner.confirm(123, 123)
        owner.close()
        self.assertFalse(any(call[0] == "terminate" for call in api.calls))

    def test_nonzero_normal_exit_is_not_accepted(self):
        api = API([False, True], exit_code=9)
        owner = self.owner(api)
        owner.confirm(123, 123)
        with self.assertRaisesRegex(AssertionError, "exited with 9"):
            owner.finish()
        self.assertEqual(api.calls[-1], ("close", api.handle))

    def test_reused_pid_does_not_reopen_or_terminate_replacement_handle(self):
        api = API([False, False, False, True])
        owner = self.owner(api)
        owner.confirm(123, 123)
        retained = owner.handle
        api.handle = 0x200000009  # A new numeric-PID occupant is a different object.
        with self.assertRaises(subprocess.TimeoutExpired):
            owner.finish()
        self.assertEqual([call for call in api.calls if call[0] == "open"], [("open", 123)])
        self.assertEqual([call for call in api.calls if call[0] == "terminate"], [("terminate", retained)])
        self.assertEqual(api.calls[-1], ("close", retained))

    def test_invalid_pid_never_opens_a_process(self):
        for pid in [True, False, None, "123", 0, -1, 0x100000000]:
            api = API([])
            with self.assertRaises(ValueError):
                lifetime.WindowsOwner(pid, Path("/owned/symbrowse.exe"), api=api)
            self.assertEqual(api.calls, [])

    def test_wait_error_still_closes_identity_and_fails(self):
        api = API([False, OSError("wait failed"), False, True])
        owner = self.owner(api)
        owner.confirm(123, 123)
        with self.assertRaisesRegex(OSError, "wait failed"):
            owner.finish()
        self.assertIn(("terminate", api.handle), api.calls)
        self.assertEqual(api.calls[-1], ("close", api.handle))

    def test_every_native_signature_uses_handle_types_and_declared_return_types(self):
        class Function:
            pass
        class DLL:
            pass
        dll = DLL()
        names = ["OpenProcess", "QueryFullProcessImageNameW", "WaitForSingleObject",
                 "GetExitCodeProcess", "TerminateProcess", "CloseHandle"]
        for name in names:
            setattr(dll, name, Function())
        with patch.object(lifetime.ctypes, "WinDLL", return_value=dll, create=True):
            lifetime.WindowsAPI()
        self.assertIs(dll.OpenProcess.restype, lifetime.wintypes.HANDLE)
        for name in names[1:]:
            self.assertIs(getattr(dll, name).argtypes[0], lifetime.wintypes.HANDLE)
        self.assertIs(dll.WaitForSingleObject.restype, lifetime.wintypes.DWORD)


if __name__ == "__main__":
    unittest.main()
