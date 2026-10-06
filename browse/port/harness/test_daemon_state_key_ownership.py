"""Exercise lease acquisition with mocked APIs, never native Windows calls."""
import ast
import ctypes
import errno
import os
from pathlib import Path
from types import SimpleNamespace
import unittest


class Function:
    def __init__(self, name, api):
        self.name, self.api = name, api

    def __call__(self, *args):
        self.api.calls.append((self.name, args[0]))
        if self.name == "OpenProcess":
            return self.api.handle
        if self.name == "QueryFullProcessImageNameW":
            args[2].value = self.api.image
            return self.api.query_result
        if self.name == "WaitForSingleObject":
            self.api.error = 222  # Must not replace the image-query error.
            return self.api.wait_status
        if self.name == "CloseHandle":
            self.api.error = 6
            return self.api.close_result
        raise AssertionError("unconfirmed process must never be terminated")


class Tests(unittest.TestCase):
    def test_acquisition_diagnostics_and_handle_ownership(self):
        source = Path(__file__).with_name("daemon_state_key_ownership.py")
        lease = next(node for node in ast.parse(source.read_bytes()).body
                     if isinstance(node, ast.ClassDef) and node.name == "Lease")
        for queried, image, closed in [(0, "/owned/symvault.exe", 1),
                                      (0, "/owned/symvault.exe", 0),
                                      (1, "/foreign/symvault.exe", 1),
                                      (1, "/owned/symvault.exe", 1)]:
            with self.subTest(queried=queried, image=image, closed=closed):
                api = SimpleNamespace(handle=0x100000005, error=5, calls=[], wait_status=258,
                                      query_result=queried, image=image, close_result=closed)
                for name in ("OpenProcess", "QueryFullProcessImageNameW", "WaitForSingleObject",
                             "TerminateProcess", "CloseHandle"):
                    setattr(api, name, Function(name, api))
                shim = SimpleNamespace(WinDLL=lambda *args, **kwargs: api,
                    get_last_error=lambda: api.error, POINTER=ctypes.POINTER,
                    create_unicode_buffer=ctypes.create_unicode_buffer, byref=ctypes.byref)
                namespace = {"ctypes": shim, "os": SimpleNamespace(name="nt"), "Path": Path}
                exec(compile(ast.Module(body=[lease], type_ignores=[]), str(source), "exec"), namespace)
                owner = namespace["Lease"].__new__(namespace["Lease"])
                if queried and image == "/owned/symvault.exe":
                    owner.__init__(123, Path(image))
                    self.assertEqual(owner.handle, api.handle)
                    self.assertFalse(any(name == "CloseHandle" for name, _ in api.calls))
                    api.wait_status = 0  # Confirmed provider has exited.
                    api.close_result = 0
                    with self.assertRaises(AssertionError) as failure:
                        owner.cleanup()
                    self.assertEqual(failure.exception.args[0], ("CloseHandle", 123, api.handle, 6))
                    self.assertEqual(owner.handle, api.handle)
                    api.close_result = 1
                    owner.cleanup()
                    self.assertIsNone(owner.handle)
                    calls = list(api.calls)
                    owner.cleanup()
                    self.assertEqual(api.calls, calls)
                else:
                    with self.assertRaises(AssertionError) as failure:
                        owner.__init__(123, Path("/owned/symvault.exe"))
                    error = failure.exception
                    if not closed:
                        self.assertEqual(error.args[0], ("CloseHandle", 123, api.handle, 6))
                        error = error.__cause__
                        assert error is not None
                        self.assertEqual(owner.handle, api.handle)
                    else:
                        self.assertIsNone(owner.handle)
                    if not queried:
                        self.assertEqual(error.args[0], {"api": "QueryFullProcessImageNameW",
                            "pid": 123, "handle": api.handle, "result": 0, "winerror": 5,
                            "wait_status": 258, "expected_executable": str(Path("/owned/symvault.exe"))})
                    self.assertEqual(api.calls[-1], ("CloseHandle", api.handle))
                self.assertFalse(any(name == "TerminateProcess" for name, _ in api.calls))
                from ctypes import wintypes
                self.assertIs(api.OpenProcess.restype, wintypes.HANDLE)
                self.assertIs(api.WaitForSingleObject.restype, wintypes.DWORD)
                for name in ("QueryFullProcessImageNameW", "TerminateProcess", "CloseHandle"):
                    self.assertIs(getattr(api, name).restype, wintypes.BOOL)

        with self.subTest(exited_before_image_query=True):
            # An unreadable image whose process then exits is a dead lease:
            # retained handle, never terminated, closed exactly once.
            api = SimpleNamespace(handle=0x100000005, error=31, calls=[], wait_status=0,
                                  query_result=0, image="", close_result=1)
            for name in ("OpenProcess", "QueryFullProcessImageNameW", "WaitForSingleObject",
                         "TerminateProcess", "CloseHandle"):
                setattr(api, name, Function(name, api))
            shim = SimpleNamespace(WinDLL=lambda *args, **kwargs: api,
                get_last_error=lambda: api.error, POINTER=ctypes.POINTER,
                create_unicode_buffer=ctypes.create_unicode_buffer, byref=ctypes.byref)
            namespace = {"ctypes": shim, "os": SimpleNamespace(name="nt"), "Path": Path}
            exec(compile(ast.Module(body=[lease], type_ignores=[]), str(source), "exec"), namespace)
            owner = namespace["Lease"](123, Path("/owned/symvault.exe"))
            self.assertEqual(owner.handle, api.handle)
            self.assertFalse(owner.alive())
            owner.cleanup()
            self.assertIsNone(owner.handle)
            self.assertEqual([name for name, _ in api.calls].count("CloseHandle"), 1)
            self.assertFalse(any(name == "TerminateProcess" for name, _ in api.calls))

        for image in ("/owned/symvault", "/foreign/symvault"):
            with self.subTest(pidfd_image=image):
                closes = []
                namespace = {"os": SimpleNamespace(name="posix", pidfd_open=lambda pid: 17,
                    readlink=lambda path: image, close=closes.append), "Path": Path}
                exec(compile(ast.Module(body=[lease], type_ignores=[]), str(source), "exec"), namespace)
                owner = namespace["Lease"].__new__(namespace["Lease"])
                if image == "/owned/symvault":
                    owner.__init__(123, Path(image))
                    self.assertEqual(owner.handle, 17)
                    self.assertEqual(closes, [])
                else:
                    with self.assertRaises(AssertionError):
                        owner.__init__(123, Path("/owned/symvault"))
                    self.assertIsNone(owner.handle)
                    self.assertEqual(closes, [17])

        observe = next(node for node in ast.parse(source.read_bytes()).body
                       if isinstance(node, ast.FunctionDef) and node.name == "observe")
        acquire = next(node for node in ast.walk(observe)
                       if isinstance(node, ast.For) and isinstance(node.target, ast.Name)
                       and node.target.id == "row")
        cleanup = next(node for node in ast.walk(observe)
                       if isinstance(node, ast.For) and isinstance(node.target, ast.Name)
                       and node.target.id == "lease")
        released = []
        retained = SimpleNamespace(cleanup=lambda: released.append("first"))
        def lease_factory(pid, executable):
            if pid == 1:
                return retained
            raise AssertionError("image query failed")
        namespace = {"rows": [{"pid": 1}, {"pid": 2}], "leases": [], "Lease": lease_factory,
                     "executable": Path("/owned/symvault"), "mode": "descendant", "trigger": "closed-writer"}
        with self.assertRaisesRegex(AssertionError, "mode='descendant', trigger='closed-writer'"):
            exec(compile(ast.Module(body=[acquire], type_ignores=[]), str(source), "exec"), namespace)
        self.assertEqual(namespace["leases"], [retained])
        exec(compile(ast.Module(body=[cleanup], type_ignores=[]), str(source), "exec"), namespace)
        self.assertEqual(released, ["first"])


    def test_darwin_identity_uses_proc_pidpath(self):
        source = Path(__file__).with_name("daemon_state_key_ownership.py")
        lease = next(node for node in ast.parse(source.read_bytes()).body
                     if isinstance(node, ast.ClassDef) and node.name == "Lease")
        for actual, result, error, expected in [
            ("/owned/symvault", 16, 0, "alive"),
            ("/foreign/symvault", 18, 0, "reject"),
            ("", 0, errno.ESRCH, "gone"),
        ]:
            with self.subTest(actual=actual, expected=expected):
                def query(pid, buffer, size):
                    self.assertEqual(pid, 123)
                    buffer.value = actual.encode()
                    return result
                api = SimpleNamespace(proc_pidpath=query)
                shim = SimpleNamespace(CDLL=lambda *args, **kwargs: api,
                    c_int=ctypes.c_int, c_void_p=ctypes.c_void_p, c_uint32=ctypes.c_uint32,
                    create_string_buffer=ctypes.create_string_buffer, get_errno=lambda: error)
                darwin_os = SimpleNamespace(name="posix", fsdecode=os.fsdecode)
                namespace = {"ctypes": shim, "errno": errno, "os": darwin_os,
                             "platform": SimpleNamespace(system=lambda: "Darwin"), "Path": Path}
                exec(compile(ast.Module(body=[lease], type_ignores=[]), str(source), "exec"), namespace)
                owner = namespace["Lease"](123, Path("/owned/symvault"))
                self.assertIs(api.proc_pidpath.restype, ctypes.c_int)
                if expected == "alive":
                    self.assertTrue(owner.alive())
                elif expected == "gone":
                    self.assertFalse(owner.alive())
                else:
                    with self.assertRaisesRegex(AssertionError, "proc_pidpath"):
                        owner.alive()


    def test_windows_job_owns_whole_tree_until_root_teardown(self):
        # The startup-key helper exits just after its provider; only a job
        # drained before TemporaryDirectory exit proves the cwd is released.
        source = Path(__file__).with_name("daemon_state_key_ownership.py")
        observe = next(node for node in ast.parse(source.read_bytes()).body
                       if isinstance(node, ast.FunctionDef) and node.name == "observe")
        popen = next(node for node in ast.walk(observe) if isinstance(node, ast.Call)
                     and ast.unparse(node.func) == "subprocess.Popen")
        flags = next(k.value for k in popen.keywords if k.arg == "creationflags")
        self.assertEqual(ast.unparse(flags), "0 if job is None else job.SUSPENDED")
        guarded = next(node for node in ast.walk(observe) if isinstance(node, ast.Try)
                       and any("kill_tree(child)" in ast.unparse(n) for n in node.finalbody))
        self.assertEqual(ast.unparse(guarded.body[0]), "if job is not None:\n    job.adopt(child)")
        self.assertIn("lease.cleanup()", ast.unparse(guarded.finalbody[-2]))
        self.assertEqual(ast.unparse(guarded.finalbody[-1]), "if job is not None:\n    job.finish()")


if __name__ == "__main__":
    unittest.main()
