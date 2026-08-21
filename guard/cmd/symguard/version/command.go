// Package version implements the `symguard version` command.
package version

import (
	"context"
	"fmt"
	"io"
	"os"
	"runtime"
	"time"

	"github.com/danieljustus/symaira-corekit/versionkit"
	"github.com/danieljustus/symaira-guard/internal/update"
)

// version is set at build time via ldflags.
var version = "dev"

// SchemaVersion is the versionkit handshake version.
// Bump whenever machine-readable JSON output changes incompatibly.
//
//	1 — initial handshake: {tool, version, schema_version}
const SchemaVersion = 1

// Run prints version and build information to w.
// When args contain "--json", the output is the versionkit.Info JSON payload.
func Run(args []string, w io.Writer) {
	info := versionkit.New("symguard", version, SchemaVersion)

	if hasFlag(args, "--json") {
		info.Write(w)
		fmt.Fprintln(w)
		return
	}

	fmt.Fprintln(w, info.String())
	fmt.Fprintf(w, "  go      %s\n", runtime.Version())
	fmt.Fprintf(w, "  os/arch %s/%s\n", runtime.GOOS, runtime.GOARCH)
	fmt.Fprintf(w, "  built   %s\n", buildTime())
}

func hasFlag(args []string, flag string) bool {
	for _, a := range args {
		if a == flag {
			return true
		}
	}
	return false
}

func buildTime() string {
	t, err := time.Parse(time.RFC3339, "2026-01-01T00:00:00Z")
	if err != nil {
		return "unknown"
	}
	return fmt.Sprintf("%s (compile-time placeholder)", t.Format("2006-01-02"))
}

// CheckLatest queries GitHub and prints an update notice to stderr
// if a newer release is available. Errors are silently swallowed.
func CheckLatest() {
	info := update.Check(context.Background(), version)
	if msg := update.Format(info); msg != "" {
		fmt.Fprint(os.Stderr, msg)
	}
}

// SetVersion sets the version string at build time.
// Called from main via ldflags.
func SetVersion(v string) {
	version = v
}
