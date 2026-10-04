"""Owned, deadline-bound Win32 byte-pipe transport for actual daemon probes."""
from __future__ import annotations

import ctypes
import json
import math
import time
from ctypes import wintypes


class Overlapped(ctypes.Structure):
    _fields_ = [("Internal", ctypes.c_size_t), ("InternalHigh", ctypes.c_size_t),
                ("Offset", wintypes.DWORD), ("OffsetHigh", wintypes.DWORD),
                ("hEvent", wintypes.HANDLE)]


class WindowsAPI:
    def __init__(self) -> None:
        self.dll = ctypes.WinDLL("kernel32", use_last_error=True)
        pointer = ctypes.POINTER
        signatures = {
            "CreateFileW": (wintypes.DWORD, [wintypes.LPCWSTR, wintypes.DWORD,
                wintypes.DWORD, ctypes.c_void_p, wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]),
            "CreateEventW": (wintypes.HANDLE, [ctypes.c_void_p, wintypes.BOOL, wintypes.BOOL, wintypes.LPCWSTR]),
            "CloseHandle": (wintypes.BOOL, [wintypes.HANDLE]),
            "ReadFile": (wintypes.BOOL, [wintypes.HANDLE, ctypes.c_void_p, wintypes.DWORD,
                pointer(wintypes.DWORD), pointer(Overlapped)]),
            "WriteFile": (wintypes.BOOL, [wintypes.HANDLE, ctypes.c_void_p, wintypes.DWORD,
                pointer(wintypes.DWORD), pointer(Overlapped)]),
            "WaitForSingleObject": (wintypes.DWORD, [wintypes.HANDLE, wintypes.DWORD]),
            "CancelIoEx": (wintypes.BOOL, [wintypes.HANDLE, pointer(Overlapped)]),
            "GetOverlappedResult": (wintypes.BOOL, [wintypes.HANDLE, pointer(Overlapped),
                pointer(wintypes.DWORD), wintypes.BOOL]),
        }
        for name, (result, arguments) in signatures.items():
            function = getattr(self.dll, name)
            function.restype, function.argtypes = result, arguments

    def open(self, path: str):
        handle = self.dll.CreateFileW(path, 0xC0000000, 0, None, 3, 0x40000000, None)
        if handle == ctypes.c_void_p(-1).value:
            raise ctypes.WinError(ctypes.get_last_error())
        return handle

    def close(self, handle) -> None:
        if not self.dll.CloseHandle(handle):
            raise ctypes.WinError(ctypes.get_last_error())

    def transfer(self, handle, data: bytes | int, deadline: float) -> bytes | int:
        writing = isinstance(data, bytes)
        buffer = ctypes.create_string_buffer(data, len(data)) if writing else ctypes.create_string_buffer(data)
        event = self.dll.CreateEventW(None, True, False, None)
        if not event:
            raise ctypes.WinError(ctypes.get_last_error())
        operation = Overlapped(hEvent=event)
        count = wintypes.DWORD()
        pending = False
        try:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("Windows pipe transfer deadline expired")
            function = self.dll.WriteFile if writing else self.dll.ReadFile
            if not function(handle, buffer, len(data) if writing else data, None, ctypes.byref(operation)):
                error = ctypes.get_last_error()
                if error != 997:  # ERROR_IO_PENDING
                    raise ctypes.WinError(error)
                pending = True
                result = self.dll.WaitForSingleObject(event, min(0xFFFFFFFE, max(1, math.ceil(remaining * 1000))))
                if result == 258:  # WAIT_TIMEOUT
                    raise TimeoutError("Windows pipe transfer deadline expired")
                if result != 0:
                    if result == 0xFFFFFFFF:
                        raise ctypes.WinError(ctypes.get_last_error())
                    raise OSError(f"unexpected Windows pipe wait result: {result}")
            if not self.dll.GetOverlappedResult(handle, ctypes.byref(operation), ctypes.byref(count), False):
                raise ctypes.WinError(ctypes.get_last_error())
            pending = False
            return count.value if writing else buffer.raw[:count.value]
        finally:
            if pending:
                # Cancellation must complete before releasing the buffer/OVERLAPPED.
                self.dll.CancelIoEx(handle, ctypes.byref(operation))
                self.dll.GetOverlappedResult(handle, ctypes.byref(operation), ctypes.byref(count), True)
            self.close(event)


def request(path: str, payload: bytes, limit: int, timeout: float, *, api=None) -> dict:
    api = WindowsAPI() if api is None else api
    deadline = time.monotonic() + timeout
    while True:
        try:
            handle = api.open(path)
            break
        except OSError as error:
            if getattr(error, "winerror", None) != 231:  # ERROR_PIPE_BUSY only, before writing.
                raise
            if time.monotonic() >= deadline:
                raise TimeoutError("Windows pipe remained busy before request") from error
            time.sleep(min(.02, max(0, deadline - time.monotonic())))
    try:
        try:
            offset = 0
            while offset < len(payload):
                written = api.transfer(handle, payload[offset:], deadline)
                if not written or written > len(payload) - offset:
                    raise OSError("Windows pipe write made invalid progress")
                offset += written
            response = bytearray()
            while len(response) <= limit:
                chunk = api.transfer(handle, min(65536, limit + 1 - len(response)), deadline)
                if not chunk:
                    break
                response.extend(chunk)
                if b"\n" in response:
                    line = bytes(response).split(b"\n", 1)[0]
                    if len(line) > limit:
                        break
                    return json.loads(line)
            raise AssertionError("daemon closed without a bounded JSON response")
        except OSError as error:
            # Once connected, never let readiness retry replay a possibly written request.
            raise RuntimeError(f"Windows pipe request failed after connection: {error}") from error
    finally:
        api.close(handle)
