//go:build !unix

package usage

import (
	"context"
	"os"
)

func readJSONHistoryFiles(ctx context.Context, path string, claude bool, parse func([]byte) ([]historyEvent, bool)) ([]historyEvent, int, error) {
	return readJSONHistoryFilesReset(ctx, path, claude, parse, func(string) bool { return true })
}

func readJSONHistoryFilesReset(ctx context.Context, path string, claude bool, parse func([]byte) ([]historyEvent, bool), reset func(string) bool) ([]historyEvent, int, error) {
	return nil, 0, os.ErrInvalid
}

func readHistoryFiles(context.Context, string) ([]historyEvent, int, error) {
	return nil, 0, os.ErrInvalid
}
