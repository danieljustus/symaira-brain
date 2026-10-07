import ctypes, importlib.util, os, sys, unittest
from pathlib import Path
from unittest.mock import Mock, patch
ROOT=Path('/workspace/symaira-daemon772-registry')
SPEC=importlib.util.spec_from_file_location('independent_pipe',Path(os.environ.get('PIPE_REVIEW_SOURCE',str(ROOT/'browse/port/harness/windows_pipe.py'))))
p=importlib.util.module_from_spec(SPEC);SPEC.loader.exec_module(p)
def error(code):
 e=OSError('Win32 error '+str(code));e.winerror=code;return e
class LowLevel(unittest.TestCase):
 def setUp(self):
  self.dll=Mock();self.api=p.WindowsAPI.__new__(p.WindowsAPI);self.api.dll=self.dll
  self.dll.CreateEventW.return_value=0x765432109abc;self.dll.CloseHandle.return_value=1
  self.dll.WriteFile.return_value=1;self.dll.ReadFile.return_value=1
  self.dll.WaitForSingleObject.return_value=0
  self.dll.CancelIoEx.return_value=1
  def result(handle,op,count,wait):count._obj.value=3;return 1
  self.dll.GetOverlappedResult.side_effect=result
  self.clock=patch.object(p.time,'monotonic',return_value=10);self.clock.start();self.addCleanup(self.clock.stop)
  self.winerr=patch.object(p.ctypes,'WinError',side_effect=error,create=True);self.winerr.start();self.addCleanup(self.winerr.stop)
  self.last=patch.object(p.ctypes,'get_last_error',return_value=997,create=True);self.last.start();self.addCleanup(self.last.stop)
 def assert_closed(self):self.dll.CloseHandle.assert_called_once_with(0x765432109abc)
 def test_immediate_write_retains_pointer_width_and_flags(self):
  self.assertEqual(self.api.transfer(0x123456789abc,b'abc',13),3)
  call=self.dll.WriteFile.call_args;self.assertEqual(call.args[0],0x123456789abc)
  self.assertIsNone(call.args[3]);self.assertEqual(call.args[4]._obj.hEvent,0x765432109abc)
  self.dll.WaitForSingleObject.assert_not_called();self.dll.CancelIoEx.assert_not_called();self.assert_closed()
 def test_immediate_read_uses_reported_count(self):
  def read(handle,buf,size,count,op):ctypes.memmove(buf,b'abcd',4);return 1
  self.dll.ReadFile.side_effect=read
  self.assertEqual(self.api.transfer(0x123456789abc,4,13),b'abc');self.assert_closed()
 def test_pending_success_waits_once_no_cancel(self):
  self.dll.WriteFile.return_value=0
  self.assertEqual(self.api.transfer(7,b'abc',13),3)
  self.dll.WaitForSingleObject.assert_called_once_with(0x765432109abc,3000)
  self.dll.GetOverlappedResult.assert_called_once();self.assertFalse(self.dll.GetOverlappedResult.call_args.args[-1])
  self.dll.CancelIoEx.assert_not_called();self.assert_closed()
 def test_pending_timeout_cancels_drains_same_operation_before_close(self):
  self.dll.WriteFile.return_value=0;self.dll.WaitForSingleObject.return_value=258
  with self.assertRaises(TimeoutError):self.api.transfer(7,b'abc',13)
  events=[call[0] for call in self.dll.mock_calls]
  self.assertEqual(events[-3:],['CancelIoEx','GetOverlappedResult','CloseHandle'])
  self.assertTrue(self.dll.GetOverlappedResult.call_args.args[-1])
  self.assertIs(self.dll.CancelIoEx.call_args.args[1]._obj,self.dll.GetOverlappedResult.call_args.args[1]._obj)
  self.assert_closed()
 def test_pending_wait_failed_preserves_original_error_then_drains(self):
  self.dll.ReadFile.return_value=0;self.dll.WaitForSingleObject.return_value=0xffffffff
  with patch.object(p.ctypes,'get_last_error',side_effect=[997,6]):
   with self.assertRaises(OSError)as caught:self.api.transfer(7,10,13)
  self.assertEqual(caught.exception.winerror,6);self.dll.CancelIoEx.assert_called_once();self.assert_closed()
 def test_pending_unexpected_wait_result_drains(self):
  self.dll.WriteFile.return_value=0;self.dll.WaitForSingleObject.return_value=0x80
  with self.assertRaisesRegex(OSError,'unexpected'):self.api.transfer(7,b'abc',13)
  self.dll.CancelIoEx.assert_called_once();self.assertTrue(self.dll.GetOverlappedResult.call_args.args[-1]);self.assert_closed()
 def test_immediate_io_error_not_cancelled(self):
  self.dll.WriteFile.return_value=0
  with patch.object(p.ctypes,'get_last_error',return_value=5):
   with self.assertRaises(OSError)as caught:self.api.transfer(7,b'abc',13)
  self.assertEqual(caught.exception.winerror,5);self.dll.CancelIoEx.assert_not_called();self.dll.GetOverlappedResult.assert_not_called();self.assert_closed()
 def test_expired_transfer_does_not_start_io(self):
  with self.assertRaises(TimeoutError):self.api.transfer(7,b'abc',10)
  self.dll.WriteFile.assert_not_called();self.dll.CancelIoEx.assert_not_called();self.assert_closed()
 def test_event_creation_error_does_not_close_invalid_handle(self):
  self.dll.CreateEventW.return_value=0
  with patch.object(p.ctypes,'get_last_error',return_value=8):
   with self.assertRaises(OSError)as caught:self.api.transfer(7,b'abc',13)
  self.assertEqual(caught.exception.winerror,8);self.dll.CloseHandle.assert_not_called();self.dll.WriteFile.assert_not_called()
 def test_result_failure_after_pending_cancels_and_drains(self):
  self.dll.WriteFile.return_value=0
  self.dll.GetOverlappedResult.side_effect=[0,1]
  with patch.object(p.ctypes,'get_last_error',side_effect=[997,109]):
   with self.assertRaises(OSError)as caught:self.api.transfer(7,b'abc',13)
  self.assertEqual(caught.exception.winerror,109);self.assertEqual(self.dll.GetOverlappedResult.call_count,2)
  self.dll.CancelIoEx.assert_called_once();self.assert_closed()
 def test_cancel_race_still_drains_completed_operation(self):
  self.dll.WriteFile.return_value=0;self.dll.WaitForSingleObject.return_value=258;self.dll.CancelIoEx.return_value=0
  with self.assertRaises(TimeoutError):self.api.transfer(7,b'abc',13)
  self.dll.GetOverlappedResult.assert_called_once();self.assertTrue(self.dll.GetOverlappedResult.call_args.args[-1]);self.assert_closed()
 def test_open_arguments_and_invalid_pointer_sentinel(self):
  self.dll.CreateFileW.return_value=0x123456789abc
  self.assertEqual(self.api.open('endpoint'),0x123456789abc)
  self.dll.CreateFileW.assert_called_once_with('endpoint',0xc0000000,0,None,3,0x40000000,None)
  self.dll.CreateFileW.return_value=ctypes.c_void_p(-1).value
  with patch.object(p.ctypes,'get_last_error',return_value=231):
   with self.assertRaises(OSError)as caught:self.api.open('endpoint')
  self.assertEqual(caught.exception.winerror,231)
 def test_function_signatures_assign_exact_native_types(self):
  with patch.object(p.ctypes,'WinDLL',return_value=self.dll,create=True)as load:
   api=p.WindowsAPI()
  load.assert_called_once_with('kernel32',use_last_error=True)
  self.assertIs(self.dll.CreateFileW.restype,p.wintypes.HANDLE)
  self.assertEqual(len(self.dll.CreateFileW.argtypes),7)
  self.assertIs(self.dll.GetOverlappedResult.argtypes[0],p.wintypes.HANDLE)
  self.assertEqual(self.dll.GetOverlappedResult.argtypes[1],ctypes.POINTER(p.Overlapped))
class RequestBoundary(unittest.TestCase):
 def test_errno22_without_winerror_is_never_retried(self):
  api=Mock();original=OSError(22,'Invalid argument');api.open.side_effect=original
  with patch.object(p.time,'monotonic',side_effect=[0,3]):
   with self.assertRaises(OSError)as caught:p.request('endpoint',b'abc',10,3,api=api)
  self.assertIs(caught.exception,original);api.open.assert_called_once();api.transfer.assert_not_called()
 def test_low_level_timeout_is_wrapped_so_readiness_cannot_replay(self):
  api=Mock();api.open.return_value=3;api.transfer.side_effect=TimeoutError('pending cancellation')
  with self.assertRaises(RuntimeError)as caught:p.request('endpoint',b'abc',10,3,api=api)
  self.assertIsInstance(caught.exception.__cause__,TimeoutError);api.open.assert_called_once();api.close.assert_called_once_with(3)
if __name__=='__main__':unittest.main(verbosity=2)
