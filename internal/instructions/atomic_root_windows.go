//go:build windows

package instructions

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"unsafe"

	"golang.org/x/sys/windows"
)

type windowsAtomicParent struct {
	handle windows.Handle
}

const (
	directoryTraverseAccess    = windows.FILE_LIST_DIRECTORY | windows.FILE_TRAVERSE | windows.FILE_READ_ATTRIBUTES | windows.READ_CONTROL | windows.SYNCHRONIZE
	directoryCreateChildAccess = directoryTraverseAccess | windows.FILE_APPEND_DATA
	directoryWriteAccess       = windows.FILE_GENERIC_READ | windows.FILE_GENERIC_WRITE | windows.SYNCHRONIZE
)

func openAtomicParent(trustedRoot, parentName string, createParent bool) (atomicParent, error) {
	rootPath, err := filepath.Abs(trustedRoot)
	if err != nil {
		return nil, err
	}
	root, err := openWindowsRootNoFollow(rootPath, createParent, parentName == ".")
	if err != nil {
		return nil, err
	}
	current := &windowsAtomicParent{handle: root}
	if parentName == "." {
		return current, nil
	}
	components := splitParent(parentName)
	for index, component := range components {
		next, err := current.openDirectory(component, createParent, index == len(components)-1)
		if err != nil {
			_ = current.Close()
			return nil, err
		}
		_ = current.Close()
		current = next
	}
	return current, nil
}

func openWindowsRootNoFollow(rootPath string, create, writableEndpoint bool) (windows.Handle, error) {
	volume := filepath.VolumeName(rootPath)
	anchorPath := volume + string(filepath.Separator)
	if volume == "" {
		anchorPath = string(filepath.Separator)
	}
	current, err := openWindowsDirectory(absoluteNTPath(anchorPath), directoryTraverseAccess, windows.FILE_OPEN)
	if err != nil {
		return windows.InvalidHandle, err
	}
	remainder := strings.TrimPrefix(rootPath, volume)
	components := strings.FieldsFunc(remainder, func(r rune) bool { return r == '\\' || r == '/' })
	for index, component := range components {
		if component == "." {
			continue
		}
		next, openErr := (&windowsAtomicParent{handle: current}).openDirectory(component, create, writableEndpoint && index == len(components)-1)
		if openErr != nil {
			_ = windows.CloseHandle(current)
			return windows.InvalidHandle, openErr
		}
		if closeErr := windows.CloseHandle(current); closeErr != nil {
			_ = next.Close()
			return windows.InvalidHandle, closeErr
		}
		current = next.handle
	}
	return current, nil
}

func splitParent(parent string) []string {
	return strings.Split(parent, "/")
}

func absoluteNTPath(path string) string {
	if strings.HasPrefix(path, `\\`) {
		return `\??\UNC` + path[1:]
	}
	return `\??\` + path
}

func ntStatusError(op, name string, status error) error {
	if ntStatus, ok := status.(windows.NTStatus); ok {
		status = ntStatus.Errno()
	}
	return &os.PathError{Op: op, Path: name, Err: status}
}

func ntCreate(root windows.Handle, name string, access, disposition, options uint32) (windows.Handle, error) {
	objectName, err := windows.NewNTUnicodeString(name)
	if err != nil {
		return windows.InvalidHandle, err
	}
	attributes := &windows.OBJECT_ATTRIBUTES{
		Length:        uint32(unsafe.Sizeof(windows.OBJECT_ATTRIBUTES{})),
		RootDirectory: root,
		ObjectName:    objectName,
		Attributes:    windows.OBJ_CASE_INSENSITIVE | windows.OBJ_DONT_REPARSE,
	}
	var statusBlock windows.IO_STATUS_BLOCK
	var allocationSize int64
	var handle windows.Handle
	status := windows.NtCreateFile(
		&handle,
		access,
		attributes,
		&statusBlock,
		&allocationSize,
		0,
		windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE|windows.FILE_SHARE_DELETE,
		disposition,
		options,
		0,
		0,
	)
	if status != nil {
		return windows.InvalidHandle, ntStatusError("open", name, status)
	}
	return handle, nil
}

func openWindowsDirectory(path string, access, disposition uint32) (windows.Handle, error) {
	return ntCreate(windows.InvalidHandle, path, access, disposition, windows.FILE_DIRECTORY_FILE|windows.FILE_OPEN_REPARSE_POINT|windows.FILE_SYNCHRONOUS_IO_NONALERT)
}

func (p *windowsAtomicParent) openDirectory(name string, create, writable bool) (*windowsAtomicParent, error) {
	access := uint32(directoryTraverseAccess)
	if writable {
		access = directoryWriteAccess
	}
	handle, err := ntCreate(p.handle, name, access, windows.FILE_OPEN, windows.FILE_DIRECTORY_FILE|windows.FILE_OPEN_REPARSE_POINT|windows.FILE_SYNCHRONOUS_IO_NONALERT)
	if err != nil && create {
		creator, creatorErr := ntCreate(p.handle, ".", directoryCreateChildAccess, windows.FILE_OPEN, windows.FILE_DIRECTORY_FILE|windows.FILE_OPEN_REPARSE_POINT|windows.FILE_SYNCHRONOUS_IO_NONALERT)
		if creatorErr != nil {
			return nil, creatorErr
		}
		handle, err = ntCreate(creator, name, access, windows.FILE_OPEN_IF, windows.FILE_DIRECTORY_FILE|windows.FILE_OPEN_REPARSE_POINT|windows.FILE_SYNCHRONOUS_IO_NONALERT)
		if closeErr := windows.CloseHandle(creator); closeErr != nil {
			if handle != windows.InvalidHandle {
				_ = windows.CloseHandle(handle)
			}
			return nil, closeErr
		}
	}
	if err != nil {
		return nil, err
	}
	info, err := windowsHandleInfo(handle)
	if err != nil {
		_ = windows.CloseHandle(handle)
		return nil, err
	}
	if info.mode&os.ModeSymlink != 0 || !info.mode.IsDir() {
		_ = windows.CloseHandle(handle)
		return nil, fmt.Errorf("atomic parent %s is not a regular directory", name)
	}
	return &windowsAtomicParent{handle: handle}, nil
}

func (p *windowsAtomicParent) Close() error {
	return windows.CloseHandle(p.handle)
}

func windowsHandleInfo(handle windows.Handle) (atomicFileInfo, error) {
	var info windows.ByHandleFileInformation
	if err := windows.GetFileInformationByHandle(handle, &info); err != nil {
		return atomicFileInfo{}, err
	}
	mode := os.FileMode(0)
	if info.FileAttributes&windows.FILE_ATTRIBUTE_REPARSE_POINT != 0 {
		mode |= os.ModeSymlink
	} else if info.FileAttributes&windows.FILE_ATTRIBUTE_DIRECTORY != 0 {
		mode |= os.ModeDir
	} else {
		mode |= 0
	}
	size := int64(uint64(info.FileSizeHigh)<<32 | uint64(info.FileSizeLow))
	return atomicFileInfo{mode: mode, size: size}, nil
}

func (p *windowsAtomicParent) openTarget(name string, access uint32, disposition uint32) (windows.Handle, error) {
	return ntCreate(p.handle, name, access, disposition, windows.FILE_NON_DIRECTORY_FILE|windows.FILE_OPEN_REPARSE_POINT|windows.FILE_SYNCHRONOUS_IO_NONALERT)
}

func (p *windowsAtomicParent) Lstat(name string) (atomicFileInfo, error) {
	handle, err := p.openTarget(name, windows.FILE_READ_ATTRIBUTES|windows.SYNCHRONIZE, windows.FILE_OPEN)
	if err != nil {
		return atomicFileInfo{}, err
	}
	defer windows.CloseHandle(handle)
	return windowsHandleInfo(handle)
}

func (p *windowsAtomicParent) OpenRead(name string) (*os.File, error) {
	handle, err := p.openTarget(name, windows.FILE_GENERIC_READ|windows.SYNCHRONIZE, windows.FILE_OPEN)
	if err != nil {
		return nil, err
	}
	info, err := windowsHandleInfo(handle)
	if err != nil || info.mode&os.ModeSymlink != 0 || !info.mode.IsRegular() {
		_ = windows.CloseHandle(handle)
		if err != nil {
			return nil, err
		}
		return nil, fmt.Errorf("target is not a regular file")
	}
	return os.NewFile(uintptr(handle), name), nil
}

func (p *windowsAtomicParent) CreateTemp(name string) (*os.File, error) {
	handle, err := p.openTarget(name, windows.FILE_GENERIC_READ|windows.FILE_GENERIC_WRITE|windows.DELETE|windows.WRITE_DAC|windows.WRITE_OWNER|windows.SYNCHRONIZE, windows.FILE_CREATE)
	if err != nil {
		return nil, err
	}
	return os.NewFile(uintptr(handle), name), nil
}

func (p *windowsAtomicParent) Remove(name string) error {
	handle, err := p.openTarget(name, windows.DELETE|windows.SYNCHRONIZE, windows.FILE_OPEN)
	if err != nil {
		return err
	}
	defer windows.CloseHandle(handle)
	deleteInfo := struct{ Delete uint8 }{Delete: 1}
	var statusBlock windows.IO_STATUS_BLOCK
	status := windows.NtSetInformationFile(handle, &statusBlock, (*byte)(unsafe.Pointer(&deleteInfo)), uint32(unsafe.Sizeof(deleteInfo)), windows.FileDispositionInformation)
	if status != nil {
		return ntStatusError("remove", name, status)
	}
	return nil
}

func (p *windowsAtomicParent) Rename(oldName, newName string) error {
	handle, err := p.openTarget(oldName, windows.DELETE|windows.SYNCHRONIZE, windows.FILE_OPEN)
	if err != nil {
		return err
	}
	defer windows.CloseHandle(handle)
	name, err := windows.UTF16FromString(newName)
	if err != nil {
		return err
	}
	const headerSize = unsafe.Offsetof(struct {
		ReplaceIfExists uint32
		RootDirectory   windows.Handle
		FileNameLength  uint32
		FileName        [1]uint16
	}{}.FileName)
	buffer := make([]byte, int(headerSize)+len(name)*2)
	info := (*struct {
		ReplaceIfExists uint32
		RootDirectory   windows.Handle
		FileNameLength  uint32
		FileName        [1]uint16
	})(unsafe.Pointer(&buffer[0]))
	info.ReplaceIfExists = 1
	info.RootDirectory = p.handle
	info.FileNameLength = uint32((len(name) - 1) * 2)
	copy(unsafe.Slice(&info.FileName[0], len(name)), name)
	var statusBlock windows.IO_STATUS_BLOCK
	status := windows.NtSetInformationFile(handle, &statusBlock, &buffer[0], uint32(len(buffer)), windows.FileRenameInformation)
	if status != nil {
		return ntStatusError("rename", newName, status)
	}
	return nil
}

func (p *windowsAtomicParent) Sync() error {
	if err := windows.FlushFileBuffers(p.handle); err != nil {
		// Windows does not support flushing directory handles on every
		// filesystem. The rename has already committed at this point, so an
		// unsupported directory flush must not turn a successful replacement
		// into a rollback-looking error. File contents were flushed before the
		// rename; directory-entry durability is therefore a documented Windows
		// limitation when this API is unavailable.
		if isUnsupportedDirectoryFlush(err) {
			return nil
		}
		return fmt.Errorf("directory replacement committed; directory flush failed: %w", err)
	}
	return nil
}

func isUnsupportedDirectoryFlush(err error) bool {
	code, ok := err.(windows.Errno)
	if !ok {
		return false
	}
	switch code {
	case windows.ERROR_INVALID_FUNCTION, windows.ERROR_INVALID_HANDLE, windows.ERROR_NOT_SUPPORTED, windows.ERROR_CALL_NOT_IMPLEMENTED:
		return true
	default:
		return false
	}
}

func (p *windowsAtomicParent) SetMode(_ *os.File, _ uint32) error {
	return nil
}

func (p *windowsAtomicParent) CopyMetadata(name string, destination *os.File) error {
	// Ordinary writes deliberately copy owner/group/DACL only. Reading or
	// writing a SACL requires SeSecurityPrivilege and ACCESS_SYSTEM_SECURITY;
	// requesting either would make normal-user replacements fail. SACL
	// preservation is an explicit privileged contract, not an ambient default.
	source, err := p.openTarget(name, windows.FILE_GENERIC_READ|windows.SYNCHRONIZE, windows.FILE_OPEN)
	if err != nil {
		return err
	}
	defer windows.CloseHandle(source)
	info := windows.SECURITY_INFORMATION(windows.OWNER_SECURITY_INFORMATION | windows.GROUP_SECURITY_INFORMATION | windows.DACL_SECURITY_INFORMATION)
	descriptor, err := windows.GetSecurityInfo(source, windows.SE_FILE_OBJECT, info)
	if err != nil {
		return fmt.Errorf("read target owner/group/DACL: %w", err)
	}
	defer func() {
		_, _ = windows.LocalFree(windows.Handle(uintptr(unsafe.Pointer(descriptor))))
	}()
	owner, _, err := descriptor.Owner()
	if err != nil {
		return fmt.Errorf("read target owner: %w", err)
	}
	group, _, err := descriptor.Group()
	if err != nil {
		return fmt.Errorf("read target group: %w", err)
	}
	dacl, _, err := descriptor.DACL()
	if err != nil {
		return fmt.Errorf("read target DACL: %w", err)
	}
	control, _, err := descriptor.Control()
	if err != nil {
		return fmt.Errorf("read target security controls: %w", err)
	}
	if control&windows.SE_DACL_PROTECTED != 0 {
		info |= windows.PROTECTED_DACL_SECURITY_INFORMATION
	}
	if err := windows.SetSecurityInfo(windows.Handle(destination.Fd()), windows.SE_FILE_OBJECT, info, owner, group, dacl, nil); err != nil {
		return fmt.Errorf("write target owner/group/DACL: %w", err)
	}
	return nil
}
