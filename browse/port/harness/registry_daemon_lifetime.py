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
import time

from registry_cli_process import CLI_TIMEOUT, CLEANUP_TIMEOUT
from registry_progress import event


class JobAccounting(ctypes.Structure):
    _fields_ = [(name, wintypes.LARGE_INTEGER) for name in
                ("TotalUserTime", "TotalKernelTime", "ThisPeriodTotalUserTime", "ThisPeriodTotalKernelTime")]
    _fields_ += [(name, wintypes.DWORD) for name in
                 ("TotalPageFaultCount", "TotalProcesses", "ActiveProcesses", "TotalTerminatedProcesses")]


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
            "CreateJobObjectW": (wintypes.HANDLE, [wintypes.LPVOID, wintypes.LPCWSTR]),
            "AssignProcessToJobObject": (wintypes.BOOL, [wintypes.HANDLE, wintypes.HANDLE]),
            "QueryInformationJobObject": (wintypes.BOOL, [wintypes.HANDLE, ctypes.c_int, wintypes.LPVOID, wintypes.DWORD, ctypes.POINTER(wintypes.DWORD)]),
            "TerminateJobObject": (wintypes.BOOL, [wintypes.HANDLE, wintypes.UINT]),
        }
        for name, (result, arguments) in signatures.items():
            function = getattr(self.dll, name)
            function.restype, function.argtypes = result, arguments
        # Documented kernel32 offers no process-wide resume for a suspended
        # Popen child (its thread handle is closed); ntdll's is long-stable.
        self.resume_process = ctypes.WinDLL("ntdll").NtResumeProcess
        self.resume_process.restype, self.resume_process.argtypes = ctypes.c_long, [wintypes.HANDLE]

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

    def job_create(self):
        handle = self.dll.CreateJobObjectW(None, None)
        if not handle:
            raise ctypes.WinError(ctypes.get_last_error())
        return handle

    def job_assign(self, job, process):
        if not self.dll.AssignProcessToJobObject(job, process):
            raise ctypes.WinError(ctypes.get_last_error())

    def resume(self, process):
        status = self.resume_process(process)
        if status:
            raise OSError(f"NtResumeProcess failed: NTSTATUS {status & 0xFFFFFFFF:#010x}")

    def job_active(self, job):
        info = JobAccounting()  # JobObjectBasicAccountingInformation == 1
        if not self.dll.QueryInformationJobObject(job, 1, ctypes.byref(info), ctypes.sizeof(info), None):
            raise ctypes.WinError(ctypes.get_last_error())
        return info.ActiveProcesses

    def job_terminate(self, job):
        if not self.dll.TerminateJobObject(job, 1):
            raise ctypes.WinError(ctypes.get_last_error())


class WindowsJob:
    """Own every process an owned CLI starts, including detached autostart
    daemons that lost the endpoint race and are never reported by PID.

    CLIs start suspended and join before running, so no descendant escapes.
    The private cwd is removed only after the job is empty; forced cleanup
    terminates the job object, never a PID or image name, and fails the gate.
    """
    SUSPENDED = 0x4  # CREATE_SUSPENDED

    def __init__(self, progress=None, *, api=None):
        self.progress = progress
        self.api = WindowsAPI() if api is None else api
        self.handle = self.api.job_create()

    def adopt(self, child):
        process = int(child._handle)
        self.api.job_assign(self.handle, process)
        self.api.resume(process)

    def _wait_empty(self, seconds):
        deadline = time.monotonic() + seconds
        while True:
            active = self.api.job_active(self.handle)
            if not active or time.monotonic() >= deadline:
                return active
            time.sleep(.02)

    def finish(self, timeout=CLI_TIMEOUT):
        try:
            event(self.progress, "job.drain.begin", timeout_seconds=timeout)
            active = self._wait_empty(timeout)
            if not active:
                event(self.progress, "job.drain.end")
                return
            event(self.progress, "job.cleanup.begin", active=active, timeout_seconds=CLEANUP_TIMEOUT)
            self.api.job_terminate(self.handle)
            remaining = self._wait_empty(CLEANUP_TIMEOUT)
            event(self.progress, "job.cleanup.end", active=remaining)
            if remaining:
                raise AssertionError(f"{remaining} owned job process(es) survived forced cleanup")
            raise AssertionError(f"{active} owned descendant process(es) outlived the case")
        finally:
            if self.handle is not None:
                self.api.close(self.handle)
                self.handle = None


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
            handle = self.handle
            self.api.close(handle)
            self.handle = None
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
