//go:build !unix

package usage

import (
	"context"
	"os"
)

func readHistoryFiles(context.Context, string) ([]historyEvent, int, error) {
	return nil, 0, os.ErrInvalid
}
