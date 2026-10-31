//go:build !unix

package usage

import (
	"errors"
	"os"
)

func openCodeReadAuthFile(_ string) ([]byte, error) {
	// shortcut: native nonblocking regular-file admission is Unix-only; other
	// platforms must supply an explicit API key until a safe opener is verified.
	return nil, errors.New("native OpenCode auth.json unsupported on this platform; set OPENCODE_GO_API_KEY explicitly")
}
func openCodeReadDir(_ string) ([]os.DirEntry, error) {
	return nil, errors.New("native OpenCode credentials unsupported on this platform; set OPENCODE_GO_API_KEY explicitly")
}

func openCodeDatabaseKey(_, _ string) (string, bool, error) {
	return "", true, errors.New("native OpenCode database unsupported")
}
