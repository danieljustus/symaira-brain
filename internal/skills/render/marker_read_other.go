//go:build !unix

package render

import (
	"io"
	"os"

	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

func readMarkerBytesNoFollow(root *os.Root, path string) ([]byte, error) {
	file, err := root.OpenFile(path, os.O_RDONLY, 0)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() || info.Size() > skill.MaxInputSize {
		return nil, errMarkerTooLarge
	}
	data, err := io.ReadAll(io.LimitReader(file, skill.MaxInputSize+1))
	if err != nil {
		return nil, err
	}
	if int64(len(data)) > skill.MaxInputSize {
		return nil, errMarkerTooLarge
	}
	return data, nil
}
