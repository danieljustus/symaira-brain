"""Portable transport controls; actual Win32 acceptance remains native CI."""
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import Mock, patch

SPEC = importlib.util.spec_from_file_location("windows_pipe", Path(__file__).with_name("windows_pipe.py"))
pipe = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(pipe)


def winerror(code):
    error = OSError(f"Win32 error {code}")
    error.winerror = code
    return error


class PipeTests(unittest.TestCase):
    def api(self):
        api = Mock()
        api.open.return_value = 0x123456789ABC
        api.transfer.side_effect = [3, b'{"ok":', b'true}\n']
        return api

    def test_busy_retries_before_one_payload_and_closes_pointer_width_handle(self):
        api = self.api()
        api.open.side_effect = [winerror(231), api.open.return_value]
        with patch.object(pipe.time, "sleep"):
            self.assertEqual(pipe.request("endpoint", b"abc", 100, 3, api=api), {"ok": True})
        self.assertEqual(api.open.call_count, 2)
        self.assertEqual(api.transfer.call_args_list[0].args[1], b"abc")
        api.close.assert_called_once_with(0x123456789ABC)

    def test_other_open_errors_preserve_code_and_never_write(self):
        for code in (2, 5, 22, 123, 233):
            with self.subTest(code=code):
                api = self.api()
                original = winerror(code)
                api.open.side_effect = original
                with self.assertRaises(OSError) as caught:
                    pipe.request("endpoint", b"abc", 100, 3, api=api)
                self.assertIs(caught.exception, original)
                api.transfer.assert_not_called()
                api.close.assert_not_called()

    def test_busy_deadline_expires_without_payload(self):
        api = self.api()
        api.open.side_effect = winerror(231)
        with patch.object(pipe.time, "monotonic", side_effect=[0, 3]):
            with self.assertRaises(TimeoutError):
                pipe.request("endpoint", b"abc", 100, 3, api=api)
        api.transfer.assert_not_called()

    def test_partial_write_sends_only_remaining_bytes(self):
        api = self.api()
        api.transfer.side_effect = [1, 2, b'{"ok":true}\n']
        pipe.request("endpoint", b"abc", 100, 3, api=api)
        self.assertEqual([call.args[1] for call in api.transfer.call_args_list[:2]], [b"abc", b"bc"])

    def test_timeout_after_connection_cannot_trigger_readiness_replay(self):
        for results in ([TimeoutError("write")], [3, TimeoutError("read")]):
            api = self.api()
            api.transfer.side_effect = results
            with self.assertRaises(RuntimeError) as caught:
                pipe.request("endpoint", b"abc", 100, 3, api=api)
            self.assertIsInstance(caught.exception.__cause__, TimeoutError)
            api.open.assert_called_once()
            api.close.assert_called_once()

    def test_broken_pipe_is_not_a_readiness_retry(self):
        api = self.api()
        api.transfer.side_effect = [3, winerror(109)]
        with self.assertRaises(RuntimeError):
            pipe.request("endpoint", b"abc", 100, 3, api=api)
        api.close.assert_called_once()

    def test_oversized_response_is_rejected_even_when_newline_arrives(self):
        api = self.api()
        api.transfer.side_effect = [3, b'{"ok":true}\n']
        with self.assertRaises(AssertionError):
            pipe.request("endpoint", b"abc", 5, 3, api=api)
        api.close.assert_called_once()

    def test_zero_write_and_eof_are_failures_with_owned_close(self):
        for results in ([0], [3, b""]):
            api = self.api()
            api.transfer.side_effect = results
            with self.assertRaises((RuntimeError, AssertionError)):
                pipe.request("endpoint", b"abc", 100, 3, api=api)
            api.close.assert_called_once()


if __name__ == "__main__":
    unittest.main()
