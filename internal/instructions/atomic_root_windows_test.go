//go:build windows

package instructions

import (
	"errors"
	"testing"

	"golang.org/x/sys/windows"
)

func TestUnsupportedDirectoryFlushIsNonFatalAfterRename(t *testing.T) {
	for _, code := range []windows.Errno{
		windows.ERROR_INVALID_FUNCTION,
		windows.ERROR_INVALID_HANDLE,
		windows.ERROR_NOT_SUPPORTED,
		windows.ERROR_CALL_NOT_IMPLEMENTED,
	} {
		if !isUnsupportedDirectoryFlush(code) {
			t.Fatalf("error %v was not classified as unsupported directory flush", code)
		}
	}
	if isUnsupportedDirectoryFlush(errors.New("unexpected flush failure")) {
		t.Fatal("unexpected flush failure was classified as unsupported")
	}
}

func TestAncestorTraversalDoesNotRequestWriteAccess(t *testing.T) {
	writeBits := uint32(windows.FILE_WRITE_DATA | windows.FILE_APPEND_DATA | windows.FILE_WRITE_ATTRIBUTES | windows.FILE_WRITE_EA)
	if directoryTraverseAccess&writeBits != 0 {
		t.Fatalf("ancestor traversal access %#x contains write bits %#x", directoryTraverseAccess, directoryTraverseAccess&writeBits)
	}
	if directoryCreateChildAccess&windows.FILE_APPEND_DATA == 0 {
		t.Fatalf("create-parent access %#x lacks FILE_ADD_SUBDIRECTORY", directoryCreateChildAccess)
	}
	if directoryCreateChildAccess&(windows.FILE_WRITE_DATA|windows.FILE_WRITE_ATTRIBUTES|windows.FILE_WRITE_EA) != 0 {
		t.Fatalf("create-parent access %#x requests unrelated write rights", directoryCreateChildAccess)
	}
	if directoryWriteAccess&writeBits == 0 {
		t.Fatalf("final parent access %#x lacks write bits", directoryWriteAccess)
	}
}
