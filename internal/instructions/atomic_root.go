//go:build darwin || dragonfly || freebsd || linux || netbsd || openbsd || solaris || windows

package instructions

import (
	"fmt"
	"io"
	"os"
	"path"
	"strings"
)

// atomicParent is a capability to the target's containing directory. Every
// operation used by AtomicFile is relative to this capability; no operation
// re-resolves the trusted root or a parent pathname after opening it.
type atomicParent interface {
	Close() error
	Lstat(name string) (atomicFileInfo, error)
	OpenRead(name string) (*os.File, error)
	CreateTemp(name string) (*os.File, error)
	Remove(name string) error
	Rename(oldName, newName string) error
	Sync() error
	SetMode(file *os.File, mode uint32) error
	CopyMetadata(name string, destination *os.File) error
}

type atomicFileInfo struct {
	mode         os.FileMode
	size         int64
	modeBits     uint32
	preserveMode bool
}

// AtomicFile is a capability to one validated target's containing directory.
// The directory capability remains stable if an attacker renames or replaces a
// path component after OpenAtomicFile returns.
type AtomicFile struct {
	parent atomicParent
	name   string
}

// OpenAtomicFile opens trustedRoot and resolves relativeTarget to one parent
// directory capability. Missing parents are created only when createParent is
// true. The target path is always relative to trustedRoot.
func OpenAtomicFile(trustedRoot, relativeTarget string, createParent bool) (*AtomicFile, error) {
	parentName, name, err := splitAtomicTarget(relativeTarget)
	if err != nil {
		return nil, err
	}
	parent, err := openAtomicParent(trustedRoot, parentName, createParent)
	if err != nil {
		return nil, err
	}
	return &AtomicFile{parent: parent, name: name}, nil
}

// ReadBounded reads the target from the retained parent capability. It rejects
// symlinks, reparse points, special files, and files larger than the source
// limit.
func (f *AtomicFile) ReadBounded() ([]byte, error) {
	info, err := f.parent.Lstat(f.name)
	if err != nil {
		return nil, err
	}
	if info.mode&os.ModeSymlink != 0 || !info.mode.IsRegular() {
		return nil, fmt.Errorf("%s is not a regular file", f.name)
	}
	if info.size > MaxSourceFileBytes {
		return nil, fmt.Errorf("%s exceeds maximum size of %d bytes", f.name, MaxSourceFileBytes)
	}
	file, err := f.parent.OpenRead(f.name)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	data, err := io.ReadAll(io.LimitReader(file, MaxSourceFileBytes+1))
	if err != nil {
		return nil, err
	}
	if len(data) > MaxSourceFileBytes {
		return nil, fmt.Errorf("%s exceeds maximum size of %d bytes", f.name, MaxSourceFileBytes)
	}
	return data, nil
}

// Write atomically replaces the target through the retained parent
// capability. Temporary creation, rename, cleanup, and synchronization are
// all relative to the same directory capability.
func (f *AtomicFile) Write(data []byte) error {
	mode := uint32(0o600)
	preserveMetadata := false
	info, err := f.parent.Lstat(f.name)
	if err == nil {
		if info.mode&os.ModeSymlink != 0 || !info.mode.IsRegular() {
			return fmt.Errorf("refusing to replace non-regular target %s", f.name)
		}
		if info.preserveMode {
			mode = info.modeBits
			preserveMetadata = true
		}
	} else if !os.IsNotExist(err) {
		return err
	}

	base := path.Base(f.name)
	var temporary *os.File
	var tempName string
	for attempt := 0; attempt < 1000; attempt++ {
		tempName = fmt.Sprintf(".%s.symbrain-tmp-%d", base, os.Getpid())
		if attempt > 0 {
			tempName += fmt.Sprintf("-%d", attempt)
		}
		temporary, err = f.parent.CreateTemp(tempName)
		if err == nil {
			break
		}
		if !os.IsExist(err) {
			return err
		}
	}
	if temporary == nil {
		return fmt.Errorf("unable to allocate atomic temp file")
	}
	removeTemp := true
	defer func() {
		if removeTemp {
			_ = f.parent.Remove(tempName)
		}
	}()

	if preserveMetadata {
		if err := f.parent.CopyMetadata(f.name, temporary); err != nil {
			_ = temporary.Close()
			return err
		}
	} else if err := f.parent.SetMode(temporary, mode); err != nil {
		_ = temporary.Close()
		return err
	}
	for len(data) > 0 {
		n, writeErr := temporary.Write(data)
		if writeErr != nil {
			_ = temporary.Close()
			return writeErr
		}
		if n == 0 {
			_ = temporary.Close()
			return io.ErrShortWrite
		}
		data = data[n:]
	}
	if preserveMetadata {
		if err := f.parent.SetMode(temporary, mode); err != nil {
			_ = temporary.Close()
			return err
		}
	}
	if err := temporary.Sync(); err != nil {
		_ = temporary.Close()
		return err
	}
	if err := temporary.Close(); err != nil {
		return err
	}
	if err := f.parent.Rename(tempName, f.name); err != nil {
		return err
	}
	removeTemp = false
	return f.parent.Sync()
}

// WriteFileAtomic opens a trusted root and atomically writes a validated
// relative target. Callers that also read/render should use OpenAtomicFile so
// the same parent capability spans the whole operation.
func WriteFileAtomic(trustedRoot, relativeTarget string, data []byte) error {
	file, err := OpenAtomicFile(trustedRoot, relativeTarget, true)
	if err != nil {
		return err
	}
	defer file.Close()
	return file.Write(data)
}

// Close releases the retained directory capability.
func (f *AtomicFile) Close() error {
	if f == nil || f.parent == nil {
		return nil
	}
	err := f.parent.Close()
	f.parent = nil
	return err
}

func splitAtomicTarget(relativeTarget string) (string, string, error) {
	raw := relativeTarget
	invalid := func() (string, string, error) {
		return "", "", fmt.Errorf("invalid relative atomic target %q", raw)
	}
	if raw == "" || strings.ContainsRune(raw, 0) || strings.HasPrefix(raw, "/") || strings.HasPrefix(raw, "\\") {
		return invalid()
	}
	if len(raw) >= 2 && ((raw[0] >= 'A' && raw[0] <= 'Z') || (raw[0] >= 'a' && raw[0] <= 'z')) && raw[1] == ':' {
		return invalid()
	}
	normalized := strings.NewReplacer("\\", "/").Replace(raw)
	parts := strings.FieldsFunc(raw, func(r rune) bool { return r == '/' || r == '\\' })
	if len(parts) == 0 || strings.Join(parts, "/") != normalized {
		return invalid()
	}
	for _, part := range parts {
		if part == "." || part == ".." || part == "" {
			return invalid()
		}
	}
	name := parts[len(parts)-1]
	parent := "."
	if len(parts) > 1 {
		parent = path.Join(parts[:len(parts)-1]...)
	}
	return parent, name, nil
}
