// Package sourcepin verifies that fixture inputs match their pinned Go oracle.
package sourcepin

import (
	"bytes"
	"fmt"
	"os"
	"os/exec"
	"path"
	"strings"
)

// Verify compares nested-module-relative files with their bytes at commit.
// Callers run from the browse module root, so git paths use browse/<relative>.
func Verify(commit string, files []string) error {
	for _, source := range files {
		if strings.ContainsAny(source, `\:`) || path.IsAbs(source) || path.Clean(source) != source || source == "." || source == ".." {
			return fmt.Errorf("invalid pinned source path %q", source)
		}
		current, err := os.ReadFile(source)
		if err != nil {
			return fmt.Errorf("read current oracle source %s: %w", source, err)
		}
		gitPath := "browse/" + source
		cmd := exec.Command("git", "show", commit+":"+gitPath)
		pinned, err := cmd.Output()
		if err != nil {
			return fmt.Errorf("read pinned oracle source %s at %s: %w", gitPath, commit, err)
		}
		if !bytes.Equal(current, pinned) {
			return fmt.Errorf("oracle source %s differs from %s", gitPath, commit)
		}
	}
	return nil
}
