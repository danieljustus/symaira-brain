//go:build !unix

package usage

import (
	"context"
	"os"
)

func readOpenCodeHistory(context.Context, string) ([]historyEvent, int, error) {
	return nil, 0, os.ErrInvalid
}
