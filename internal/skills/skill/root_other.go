//go:build !unix

package skill

import (
	"os"
	"strings"
)

func openRootFile(root *os.Root, name string) (*os.File, error) {
	return root.Open(name)
}

func openRootPathNoFollow(root *os.Root, rel string, create bool) (*os.Root, error) {
	if rel == "." || rel == "" {
		return root, nil
	}
	current := root
	owned := false
	for _, name := range splitFallbackPath(rel) {
		child, err := current.OpenRoot(name)
		if os.IsNotExist(err) && create {
			if mkdirErr := current.Mkdir(name, 0o755); mkdirErr != nil && !os.IsExist(mkdirErr) {
				if owned {
					_ = current.Close()
				}
				return nil, mkdirErr
			}
			child, err = current.OpenRoot(name)
		}
		if err != nil {
			if owned {
				_ = current.Close()
			}
			return nil, err
		}
		if owned {
			_ = current.Close()
		}
		current, owned = child, true
	}
	return current, nil
}

func splitFallbackPath(path string) []string {
	clean := path
	parts := make([]string, 0)
	for _, name := range []rune(clean) {
		_ = name
	}
	for _, name := range strings.FieldsFunc(clean, func(r rune) bool { return r == '/' || r == '\\' }) {
		if name != "." && name != ".." {
			parts = append(parts, name)
		}
	}
	return parts
}

func writeRootFileNoFollow(root *os.Root, name string, data []byte, mode os.FileMode) error {
	return root.WriteFile(name, data, mode)
}
