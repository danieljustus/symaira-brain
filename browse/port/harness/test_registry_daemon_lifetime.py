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
                 "GetExitCodeProcess", "TerminateProcess", "CloseHandle",
                 "AssignProcessToJobObject", "QueryInformationJobObject", "TerminateJobObject"]
        for name in names + ["CreateJobObjectW", "NtResumeProcess"]:
            setattr(dll, name, Function())
        with patch.object(lifetime.ctypes, "WinDLL", return_value=dll, create=True):
            lifetime.WindowsAPI()
        self.assertIs(dll.OpenProcess.restype, lifetime.wintypes.HANDLE)
        for name in names[1:]:
            self.assertIs(getattr(dll, name).argtypes[0], lifetime.wintypes.HANDLE)
        self.assertIs(dll.WaitForSingleObject.restype, lifetime.wintypes.DWORD)
        self.assertIs(dll.CreateJobObjectW.restype, lifetime.wintypes.HANDLE)
        self.assertEqual(dll.AssignProcessToJobObject.argtypes,
                         [lifetime.wintypes.HANDLE, lifetime.wintypes.HANDLE])
        self.assertEqual(dll.NtResumeProcess.argtypes, [lifetime.wintypes.HANDLE])


class JobAPI:
    def __init__(self, active):
        self.active, self.calls, self.job = list(active), [], 0x200000007

    def job_create(self):
        self.calls.append(("create",))
        return self.job

    def job_assign(self, job, process):
        self.calls.append(("assign", job, process))

    def resume(self, process):
        self.calls.append(("resume", process))

    def job_active(self, job):
        self.calls.append(("active", job))
        return self.active.pop(0) if len(self.active) > 1 else self.active[0]

    def job_terminate(self, job):
        self.calls.append(("terminate", job))

    def job_pids(self, job):
        self.calls.append(("pids", job))
        return [4242]

    def process_image(self, pid):
        self.calls.append(("image", pid))
        return r"C:\owned\symbrowse.exe"

    def close(self, handle):
        self.calls.append(("close", handle))


class JobTests(unittest.TestCase):
    def test_adopt_assigns_suspended_child_before_resuming_it(self):
        api = JobAPI([0])
        job = lifetime.WindowsJob(api=api)
        job.adopt(type("Child", (), {"_handle": 0x300000009})())
        self.assertEqual(api.calls, [("create",), ("assign", api.job, 0x300000009), ("resume", 0x300000009)])
        self.assertEqual(job.SUSPENDED, 0x4)

    def test_resume_failure_raises_and_leaves_child_for_capture_cleanup(self):
        api = JobAPI([0])
        def failed(process):
            api.calls.append(("resume", process))
            raise OSError("NtResumeProcess failed")
        api.resume = failed
        job = lifetime.WindowsJob(api=api)
        with self.assertRaisesRegex(OSError, "NtResumeProcess failed"):
            job.adopt(type("Child", (), {"_handle": 9})())
        self.assertEqual(api.calls[-2:], [("assign", api.job, 9), ("resume", 9)])

    def test_native_resume_checks_ntstatus_with_handle_argument(self):
        api = lifetime.WindowsAPI.__new__(lifetime.WindowsAPI)
        seen = []
        def resume(handle):
            seen.append(handle)
            return status
        api.resume_process = resume
        status = 0
        api.resume(0x300000009)
        status = -1073741816  # STATUS_INVALID_HANDLE as a signed NTSTATUS.
        with self.assertRaisesRegex(OSError, "0xc0000008"):
            api.resume(0x300000009)
        self.assertEqual(seen, [0x300000009, 0x300000009])

    def test_cwd_is_released_only_after_lost_autostart_daemons_exit(self):
        api = JobAPI([2, 1, 0])
        job = lifetime.WindowsJob(api=api)
        facts = job.finish()
        self.assertEqual(facts, {"drain_seconds": 15, "survivors": [], "terminated": False})
        self.assertEqual(facts, dict(lifetime.no_job_teardown(), drain_seconds=15))
        self.assertEqual([call[0] for call in api.calls], ["create", "active", "active", "active", "close"])
        self.assertIsNone(job.handle)

    def test_survivor_is_named_then_terminated_and_reported_not_judged(self):
        api = JobAPI([1, 0])
        job = lifetime.WindowsJob(api=api)
        facts = job.finish(timeout=0)
        self.assertEqual(facts["survivors"], [{"pid": 4242, "image": r"C:\owned\symbrowse.exe"}])
        self.assertTrue(facts["terminated"])
        # Survivors are named before the job object (never a PID) is terminated.
        self.assertEqual([call[0] for call in api.calls],
                         ["create", "active", "pids", "image", "terminate", "active", "close"])

    def test_survivor_of_job_termination_fails_and_still_closes(self):
        api = JobAPI([1])
        job = lifetime.WindowsJob(api=api)
        with patch.object(lifetime, "CLEANUP_TIMEOUT", 0):
            with self.assertRaisesRegex(AssertionError, "survived forced cleanup"):
                job.finish(timeout=0)
        self.assertEqual(api.calls[-1], ("close", api.job))


class RevivedOwnerTests(unittest.TestCase):
    def test_windows_sweep_stops_each_rebound_owner_until_job_is_empty(self):
        import daemon_registry as registry
        # A straggler is still starting (no pipe), then binds and answers,
        # then the job empties once it has stopped.
        job = type("Job", (), {"states": [2, 2, 1, 0]})()
        job.active = lambda: job.states.pop(0)
        replies = [FileNotFoundError(2, "no pipe"), {"success": True, "data": {"stopping": True}},
                   FileNotFoundError(2, "stopped straggler released the pipe")]
        def request(endpoint, frame):
            self.assertEqual(frame["cmd"], "daemon.stop")
            reply = replies.pop(0)
            if isinstance(reply, BaseException):
                raise reply
            return reply
        with patch.object(registry.harness, "request", side_effect=request), \
             patch.object(registry.time, "sleep"):
            self.assertEqual(registry.stop_revived_owners(Path("owned"), "owned", job), 1)
        self.assertEqual(job.states, [])

    def test_posix_sweep_ends_after_quiet_second(self):
        import daemon_registry as registry
        clock = iter([0, 0, 0, 0.5, 0.5, 1.2, 1.2])
        with patch.object(registry.harness, "request", side_effect=ConnectionRefusedError()), \
             patch.object(registry.time, "monotonic", lambda: next(clock)), \
             patch.object(registry.time, "sleep"):
            self.assertEqual(registry.stop_revived_owners(Path("owned"), "owned", None), 0)


if __name__ == "__main__":
    unittest.main()
