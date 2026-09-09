//go:build windows

package safefs

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"unsafe"

	"golang.org/x/sys/windows"
)

// OpenConfigFile opens path as a regular, non-reparse file. Every ancestor is
// resolved through a handle with reparse-point opening disabled, so a renamed
// or substituted parent cannot redirect the read to another tree.
func OpenConfigFile(path string) (*os.File, error) {
	parent, err := openDirectoryNoFollow(filepath.Dir(path))
	if err != nil {
		return nil, &os.PathError{Op: "open", Path: path, Err: err}
	}
	defer windows.CloseHandle(parent)

	handle, err := openRelative(parent, filepath.Base(path), windows.FILE_READ_DATA|windows.FILE_READ_ATTRIBUTES|windows.SYNCHRONIZE, windows.FILE_NON_DIRECTORY_FILE|windows.FILE_OPEN_REPARSE_POINT)
	if err != nil {
		return nil, &os.PathError{Op: "open", Path: path, Err: err}
	}
	var info windows.ByHandleFileInformation
	if err := windows.GetFileInformationByHandle(handle, &info); err != nil {
		windows.CloseHandle(handle)
		return nil, &os.PathError{Op: "stat", Path: path, Err: err}
	}
	if info.FileAttributes&windows.FILE_ATTRIBUTE_REPARSE_POINT != 0 {
		windows.CloseHandle(handle)
		return nil, &os.PathError{Op: "open", Path: path, Err: fmt.Errorf("configuration target is a reparse point")}
	}
	return os.NewFile(uintptr(handle), path), nil
}

// RemoveNoFollow removes one directory entry relative to its retained parent
// handle. The final entry may be a reparse point, but no ancestor may be one.
func RemoveNoFollow(path string) error {
	parent, err := openDirectoryNoFollow(filepath.Dir(path))
	if err != nil {
		return &os.PathError{Op: "remove", Path: path, Err: err}
	}
	defer windows.CloseHandle(parent)

	handle, err := openRelative(parent, filepath.Base(path), windows.DELETE|windows.FILE_READ_ATTRIBUTES|windows.SYNCHRONIZE, windows.FILE_OPEN_REPARSE_POINT)
	if err != nil {
		return &os.PathError{Op: "remove", Path: path, Err: err}
	}
	defer windows.CloseHandle(handle)

	disposition := uint32(windows.FILE_DISPOSITION_DELETE | windows.FILE_DISPOSITION_POSIX_SEMANTICS)
	if err := windows.SetFileInformationByHandle(handle, windows.FileDispositionInfoEx, (*byte)(unsafe.Pointer(&disposition)), uint32(unsafe.Sizeof(disposition))); err != nil {
		return &os.PathError{Op: "remove", Path: path, Err: err}
	}
	return nil
}

func openDirectoryNoFollow(path string) (windows.Handle, error) {
	clean := filepath.Clean(path)
	anchor, remainder, err := splitAnchor(clean)
	if err != nil {
		return windows.InvalidHandle, err
	}
	current, err := openDirectoryPath(anchor)
	if err != nil {
		return windows.InvalidHandle, err
	}
	for _, component := range remainder {
		next, err := openRelative(current, component, windows.FILE_LIST_DIRECTORY|windows.FILE_READ_ATTRIBUTES|windows.SYNCHRONIZE, windows.FILE_DIRECTORY_FILE|windows.FILE_OPEN_REPARSE_POINT)
		windows.CloseHandle(current)
		if err != nil {
			return windows.InvalidHandle, fmt.Errorf("open configuration directory component %q: %w", component, err)
		}
		var info windows.ByHandleFileInformation
		if err := windows.GetFileInformationByHandle(next, &info); err != nil {
			windows.CloseHandle(next)
			return windows.InvalidHandle, err
		}
		if info.FileAttributes&windows.FILE_ATTRIBUTE_REPARSE_POINT != 0 {
			windows.CloseHandle(next)
			return windows.InvalidHandle, fmt.Errorf("open configuration directory component %q: reparse point", component)
		}
		current = next
	}
	return current, nil
}

func splitAnchor(path string) (string, []string, error) {
	volume := filepath.VolumeName(path)
	anchor := "."
	remainder := path
	if volume != "" {
		anchor = volume + `\`
		remainder = strings.TrimPrefix(path, volume)
	} else if filepath.IsAbs(path) {
		anchor = `\`
		remainder = strings.TrimPrefix(path, `\`)
	}
	remainder = strings.TrimLeft(remainder, `\`)
	var components []string
	for _, component := range strings.Split(remainder, `\`) {
		switch component {
		case "", ".":
			continue
		case "..":
			return "", nil, fmt.Errorf("configuration directory contains parent component")
		default:
			components = append(components, component)
		}
	}
	return anchor, components, nil
}

func openDirectoryPath(path string) (windows.Handle, error) {
	name, err := windows.UTF16PtrFromString(path)
	if err != nil {
		return windows.InvalidHandle, err
	}
	handle, err := windows.CreateFile(name, windows.FILE_LIST_DIRECTORY|windows.FILE_READ_ATTRIBUTES|windows.SYNCHRONIZE, windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE|windows.FILE_SHARE_DELETE, nil, windows.OPEN_EXISTING, windows.FILE_FLAG_BACKUP_SEMANTICS|windows.FILE_FLAG_OPEN_REPARSE_POINT, 0)
	if err != nil {
		return windows.InvalidHandle, err
	}
	var info windows.ByHandleFileInformation
	if err := windows.GetFileInformationByHandle(handle, &info); err != nil {
		windows.CloseHandle(handle)
		return windows.InvalidHandle, err
	}
	if info.FileAttributes&windows.FILE_ATTRIBUTE_REPARSE_POINT != 0 {
		windows.CloseHandle(handle)
		return windows.InvalidHandle, fmt.Errorf("configuration directory anchor is a reparse point")
	}
	return handle, nil
}

func openRelative(parent windows.Handle, name string, access, options uint32) (windows.Handle, error) {
	objectName, err := windows.NewNTUnicodeString(name)
	if err != nil {
		return windows.InvalidHandle, err
	}
	attributes := windows.OBJECT_ATTRIBUTES{
		Length:        uint32(unsafe.Sizeof(windows.OBJECT_ATTRIBUTES{})),
		RootDirectory: parent,
		ObjectName:    objectName,
		Attributes:    windows.OBJ_CASE_INSENSITIVE,
	}
	var handle windows.Handle
	var status windows.IO_STATUS_BLOCK
	err = windows.NtCreateFile(&handle, access, &attributes, &status, nil, windows.FILE_ATTRIBUTE_NORMAL, windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE|windows.FILE_SHARE_DELETE, windows.FILE_OPEN, options|windows.FILE_SYNCHRONOUS_IO_NONALERT, 0, 0)
	if err != nil {
		if ntstatus, ok := err.(windows.NTStatus); ok {
			return windows.InvalidHandle, ntstatus.Errno()
		}
		return windows.InvalidHandle, err
	}
	return handle, nil
}
