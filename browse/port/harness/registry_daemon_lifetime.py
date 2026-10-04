"""Windows detached-daemon ownership: retain an image-verified process handle.

An IPC stop reply precedes process exit. The private cwd must outlive that exit;
never remove its directory merely because the stop response arrived. Forced
cleanup releases only the retained kernel object and always fails the gate.
"""
import ctypes
from ctypes import wintypes
import math
from pathlib import Path
import subprocess

from registry_cli_process import CLI_TIMEOUT, CLEANUP_TIMEOUT
from registry_progress import event


class WindowsAPI:
    def __init__(self):
        self.dll = ctypes.WinDLL("kernel32", use_last_error=True)
        signatures = {
            "OpenProcess": (wintypes.HANDLE, [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]),
            "QueryFullProcessImageNameW": (wintypes.BOOL, [wintypes.HANDLE, wintypes.DWORD, wintypes.LPWSTR, ctypes.POINTER(wintypes.DWORD)]),
            "WaitForSingleObject": (wintypes.DWORD, [wintypes.HANDLE, wintypes.DWORD]),
            "GetExitCodeProcess": (wintypes.BOOL, [wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD)]),
            "TerminateProcess": (wintypes.BOOL, [wintypes.HANDLE, wintypes.UINT]),
            "CloseHandle": (wintypes.BOOL, [wintypes.HANDLE]),
        }
        for name, (result, arguments) in signatures.items():
            function = getattr(self.dll, name)
            function.restype, function.argtypes = result, arguments

    def open(self, pid):
        handle = self.dll.OpenProcess(0x100000 | 0x1000 | 0x1, False, pid)
        if not handle:
            raise ctypes.WinError(ctypes.get_last_error())
        return handle

    def image(self, handle):
        size = wintypes.DWORD(32768)
        name = ctypes.create_unicode_buffer(size.value)
        if not self.dll.QueryFullProcessImageNameW(handle, 0, name, ctypes.byref(size)):
            raise ctypes.WinError(ctypes.get_last_error())
        return name.value

    def wait(self, handle, seconds):
        result = self.dll.WaitForSingleObject(handle, math.ceil(seconds * 1000))
        if result not in (0, 258):
            raise ctypes.WinError(ctypes.get_last_error())
        return result == 0

    def exit_code(self, handle):
        code = wintypes.DWORD()
        if not self.dll.GetExitCodeProcess(handle, ctypes.byref(code)):
            raise ctypes.WinError(ctypes.get_last_error())
        return code.value

    def terminate(self, handle):
        if not self.dll.TerminateProcess(handle, 1) and not self.wait(handle, 0):
            raise ctypes.WinError(ctypes.get_last_error())

    def close(self, handle):
        if not self.dll.CloseHandle(handle):
            raise ctypes.WinError(ctypes.get_last_error())


class WindowsOwner:
    """The private endpoint identifies PID; the retained handle pins identity."""
    def __init__(self, pid, binary, progress=None, *, api=None):
        if type(pid) is not int or not 0 < pid <= 0xFFFFFFFF:
            raise ValueError("invalid owned daemon PID")
        self.pid, self.binary, self.progress = pid, Path(binary), progress
        self.api = WindowsAPI() if api is None else api
        self.handle = self.api.open(pid)
        self.confirmed = False
        try:
            actual = Path(self.api.image(self.handle)).resolve()
            if actual != self.binary.resolve():
                raise AssertionError(f"daemon PID image differs: {actual}")
            event(progress, "autostart.identity.retained", pid=pid, image=str(actual))
        except BaseException:
            self.close()
            raise

    def confirm(self, status_pid, info_pid):
        if status_pid != self.pid or info_pid != self.pid or self.api.wait(self.handle, 0):
            raise AssertionError("autostart process owner changed before confirmation")
        self.confirmed = True
        event(self.progress, "autostart.identity.confirmed", pid=self.pid)

    def close(self):
        if self.handle is not None:
            handle, self.handle = self.handle, None
            self.api.close(handle)
            event(self.progress, "autostart.identity.closed", pid=self.pid)

    def finish(self, stop_error=None):
        """Wait normal exit; failed stop/timeout never becomes an accepted case."""
        failure = stop_error
        try:
            if not self.confirmed:
                event(self.progress, "autostart.cleanup.refused", pid=self.pid,
                      reason="unconfirmed process identity")
                raise AssertionError("refuse cleanup of unconfirmed autostart identity")
            event(self.progress, "autostart.exit.wait.begin", pid=self.pid,
                  timeout_seconds=CLI_TIMEOUT)
            if failure is None:
                try:
                    if not self.api.wait(self.handle, CLI_TIMEOUT):
                        failure = subprocess.TimeoutExpired([str(self.binary), "daemon"], CLI_TIMEOUT)
                except BaseException as error:
                    failure = error
            if failure is not None:
                event(self.progress, "autostart.exit.failed", pid=self.pid,
                      failure=repr(failure))
                # Termination targets the kernel object retained before stop,
                # never a newly opened/reused PID or an executable-name match.
                try:
                    if not self.api.wait(self.handle, 0):
                        event(self.progress, "autostart.cleanup.begin", pid=self.pid,
                              timeout_seconds=CLEANUP_TIMEOUT)
                        self.api.terminate(self.handle)
                        if not self.api.wait(self.handle, CLEANUP_TIMEOUT):
                            raise AssertionError("owned autostart daemon survived forced cleanup")
                        event(self.progress, "autostart.cleanup.end", pid=self.pid)
                except BaseException as error:
                    event(self.progress, "autostart.cleanup.failed", pid=self.pid,
                          failure=repr(error), original_failure=repr(failure))
                    raise error from failure
                raise failure
            code = self.api.exit_code(self.handle)
            event(self.progress, "autostart.exit.wait.end", pid=self.pid, exit=code)
            if code != 0:
                raise AssertionError(f"autostart daemon exited with {code}")
        finally:
            self.close()
