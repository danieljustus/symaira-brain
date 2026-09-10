//go:build windows

package profile

import (
	"fmt"

	"github.com/danieljustus/symaira-brain/internal/safefs"
)

// Remove removes a profile through a retained, no-follow parent capability.
// The final entry itself is deleted without following a reparse point; every
// ancestor must be a real directory.
func Remove(name string) error {
	path := Path(name)
	if err := safefs.RemoveNoFollow(path); err != nil {
		return fmt.Errorf("remove %s: %w", path, err)
	}
	return nil
}
