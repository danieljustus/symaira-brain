import ctypes
import unittest
from types import SimpleNamespace
from unittest.mock import patch

import run


class EndpointTest(unittest.TestCase):
    def test_windows_go_and_rust_use_the_same_pipe(self):
        with patch.object(run, "os", SimpleNamespace(name="nt")):
            self.assertEqual(run.daemon_endpoint("bench", {}), r"\\.\pipe\symbrowse-bench")


    def test_windows_pipe_exchange_completes_successful_partial_writes(self):
        payload = b'{"cmd":"daemon.ping"}\n'
        response = b'{"success":true}\n'
        written_chunks = []

        def write_file(_handle, buffer, length, written, _overlapped):
            size = min(5, length)
            written_chunks.append(ctypes.string_at(buffer, size))
            ctypes.cast(written, ctypes.POINTER(ctypes.c_uint32)).contents.value = size
            return 1

        def read_file(_handle, buffer, _length, count, _overlapped):
            ctypes.memmove(buffer, response, len(response))
            ctypes.cast(count, ctypes.POINTER(ctypes.c_uint32)).contents.value = len(response)
            return 1

        api = (
            lambda _endpoint, _timeout: 1,
            lambda *_args: 1,
            lambda *_args: 1,
            (read_file, write_file, lambda _handle: 1),
        )
        with patch.object(run, "os", SimpleNamespace(name="nt")), patch.object(
            run, "_windows_pipe_api", return_value=api
        ):
            result = run.windows_pipe_exchange(r"\\.\pipe\symbrowse-bench", payload, 1)

        self.assertEqual(result, response)
        self.assertEqual(b"".join(written_chunks), payload)
        self.assertGreater(len(written_chunks), 1)


if __name__ == "__main__":
    unittest.main()
