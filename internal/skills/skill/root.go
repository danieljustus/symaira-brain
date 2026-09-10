package skill

import (
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"unicode/utf8"
)

// openBundleRoot opens the trusted bundle directory once. All subsequent
// reads use the returned capability, so a checked pathname cannot be swapped
// for an outside symlink before it is consumed.
func openBundleRoot(path string) (*os.Root, error) {
	root, err := os.OpenRoot(path)
	if err != nil {
		return nil, fmt.Errorf("open skill root: %w", err)
	}
	info, err := root.Stat(".")
	if err != nil {
		_ = root.Close()
		return nil, fmt.Errorf("stat skill root: %w", err)
	}
	if !info.IsDir() {
		_ = root.Close()
		return nil, fmt.Errorf("skill root %q is not a directory", path)
	}
	return root, nil
}

// OpenBundleRoot opens a bundle capability for helper operations that need to
// read the same root without pathname reopening.
func OpenBundleRoot(path string) (*os.Root, error) { return openBundleRoot(path) }

func readRootFile(root *os.Root, rel, name string, limit int64) ([]byte, error) {
	f, err := openRootFile(root, filepath.FromSlash(rel))
	if err != nil {
		return nil, fmt.Errorf("read %s: %w", name, err)
	}
	defer f.Close()
	info, err := f.Stat()
	if err != nil {
		return nil, fmt.Errorf("stat %s: %w", name, err)
	}
	if !info.Mode().IsRegular() {
		return nil, fmt.Errorf("%s must be a regular file", name)
	}
	if info.Size() > limit {
		return nil, fmt.Errorf("%s exceeds maximum input size of %d bytes", name, limit)
	}
	data, err := io.ReadAll(io.LimitReader(f, limit+1))
	if err != nil {
		return nil, fmt.Errorf("read %s: %w", name, err)
	}
	if int64(len(data)) > limit {
		return nil, fmt.Errorf("%s exceeds maximum input size of %d bytes", name, limit)
	}
	return data, nil
}

// ReadBundleBytes reads a bundle-relative file through its retained root
// capability. Callers must pass a relative slash-separated path.
func ReadRootBytes(root *os.Root, rel string, limit int64) ([]byte, error) {
	if root == nil {
		return nil, errors.New("bundle has no root capability")
	}
	if filepath.IsAbs(rel) || rel == "" {
		return nil, fmt.Errorf("bundle path %q must be relative", rel)
	}
	clean := filepath.Clean(filepath.FromSlash(rel))
	if clean == "." || clean == ".." || strings.HasPrefix(clean, ".."+string(filepath.Separator)) {
		return nil, fmt.Errorf("bundle path %q escapes skill root", rel)
	}
	return readRootFile(root, filepath.ToSlash(clean), rel, limit)
}

func readRootDir(root *os.Root, rel string) ([]os.DirEntry, error) {
	if root == nil {
		return nil, errors.New("bundle has no root capability")
	}
	if rel == "" {
		rel = "."
	}
	if filepath.IsAbs(rel) {
		return nil, fmt.Errorf("bundle path %q must be relative", rel)
	}
	dir, err := root.Open(filepath.FromSlash(rel))
	if err != nil {
		return nil, err
	}
	defer dir.Close()
	return dir.ReadDir(-1)
}

// EnsureRootPath rejects symlinked parents and creates the requested
// directory entirely below root.
func EnsureRootPath(root *os.Root, rel string) error {
	if root == nil {
		return errors.New("destination has no root capability")
	}
	if rel == "" || filepath.IsAbs(rel) {
		return fmt.Errorf("destination path %q must be relative", rel)
	}
	clean := filepath.Clean(filepath.FromSlash(rel))
	if clean == ".." || strings.HasPrefix(clean, ".."+string(filepath.Separator)) {
		return fmt.Errorf("destination path %q escapes root", rel)
	}
	if clean == "." {
		return nil
	}
	pathRoot, err := openRootPathNoFollow(root, clean, true)
	if err != nil {
		return fmt.Errorf("destination path %q: %w", rel, err)
	}
	if pathRoot != root {
		defer pathRoot.Close()
	}
	return nil
}

// WriteRootFile writes a file below root after rejecting symlinked parents and
// a symlink at the final path.
func WriteRootFile(root *os.Root, rel string, data []byte, mode os.FileMode) error {
	if root == nil {
		return errors.New("destination has no root capability")
	}
	if rel == "" || filepath.IsAbs(rel) {
		return fmt.Errorf("destination path %q must be relative", rel)
	}
	clean := filepath.Clean(filepath.FromSlash(rel))
	if clean == "." || clean == ".." || strings.HasPrefix(clean, ".."+string(filepath.Separator)) {
		return fmt.Errorf("destination path %q is not a file path", rel)
	}
	parent := filepath.Dir(clean)
	pathRoot, err := openRootPathNoFollow(root, parent, true)
	if err != nil {
		return err
	}
	if pathRoot != root {
		defer pathRoot.Close()
	}
	return writeRootFileNoFollow(pathRoot, filepath.Base(clean), data, mode)
}

func ReadBundleBytes(bundle *Bundle, rel string, limit int64) ([]byte, error) {
	if bundle == nil {
		return nil, errors.New("bundle has no root capability")
	}
	return ReadRootBytes(bundle.rootCap, rel, limit)
}

// ReadExternalBytes reads an absolute template path through a retained
// capability rooted at its parent directory. The final file is opened with
// no-follow semantics on Unix and is bounded and required to be regular.
func ReadExternalBytes(path string, limit int64) ([]byte, error) {
	if path == "" || !filepath.IsAbs(path) {
		return nil, fmt.Errorf("external path %q must be absolute", path)
	}
	abs, err := filepath.Abs(path)
	if err != nil {
		return nil, err
	}
	parent, name := filepath.Dir(abs), filepath.Base(abs)
	root, err := openBundleRoot(parent)
	if err != nil {
		return nil, err
	}
	defer root.Close()
	return ReadRootBytes(root, name, limit)
}

// ReadBundleText reads and strictly decodes a textual bundle file. It is used
// for overlays and configured prepend/append inputs, where replacement bytes
// must never be silently changed by a decoder.
func ReadBundleText(bundle *Bundle, rel string, limit int64) (string, error) {
	data, err := ReadBundleBytes(bundle, rel, limit)
	if err != nil {
		return "", err
	}
	if !utf8.Valid(data) {
		return "", fmt.Errorf("invalid_utf8_overlay: %s", rel)
	}
	return string(data), nil
}
