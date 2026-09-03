package main

import (
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"

	"github.com/danieljustus/symaira-brain/internal/xdg"
	"github.com/danieljustus/symaira-corekit/exitcodes"
	"github.com/danieljustus/symaira-corekit/fsutil"
)

const defaultConfigTOML = `# symbrain global configuration.
#
# All keys are optional; the values below are the defaults. Environment
# variables SYMBRAIN_<SECTION>_<FIELD> override this file (e.g.
# SYMBRAIN_AUDIT_ENABLED=false).

# Profile used when a command needs one but none was given explicitly.
default_profile = ""

[audit]
# JSONL audit log under ~/.local/share/symbrain/audit/<profile>.jsonl.
enabled = true
# Additionally log non-vault tool argument values (never vault arguments
# or results, regardless of this setting).
verbose = false

[gateway]
# Override mapped backend identity parameters with the active profile name.
identity_injection = true

[updatecheck]
# Check GitHub releases for newer symbrain versions.
enabled = true

# Optional per-server binary path overrides; uncomment to bypass PATH
# lookup for a specific child server.
# [servers.vault]
# binary_path = "/opt/symvault/symvault"
#
# [servers.memory]
# binary_path = ""
#
# [servers.skills]
# binary_path = ""
#
# [servers.usage]
# binary_path = ""
`

const personalProfileTOML = `# Example profile: full access for trusted personal use.
[profile]
name        = "personal"
description = "Full access for trusted personal use"

[servers.vault]
enabled = true
mode    = "full"

[servers.memory]
enabled = true
mode    = "read_write"

[servers.skills]
enabled = true

[servers.usage]
enabled = true

[audit]
enabled = true
`

const restrictedProfileTOML = `# Example profile: least-privilege for untrusted or shared harnesses.
[profile]
name        = "restricted"
description = "Least-privilege profile for untrusted or shared harnesses"

[servers.vault]
enabled = true
mode    = "request_only"

[servers.memory]
enabled = true
mode    = "read_only"

[servers.skills]
enabled = true

[servers.usage]
enabled = true

[audit]
enabled = true
`

const foreignReadOnlyProfileTOML = `# Example profile: read-only access to a foreign (non-core) MCP server,
# alongside restricted access to the four cores.
[profile]
name        = "foreign-read-only"
description = "Read-only access to a foreign server, plus the four cores"

[servers.vault]
enabled = true
mode    = "request_only"

[servers.memory]
enabled = true
mode    = "read_only"

[servers.skills]
enabled = true

[servers.usage]
enabled = true

# A foreign server — any alias beyond vault/memory/skills/usage — has no
# in-repo tool universe, so symbrain filters its live tools instead of
# gatekeeping against a preset list (see README.md "Foreign servers:
# symbrain as a filter, not a gatekeeper").
#
# access = "read" exposes only tools classified as reading. Classification
# precedence: an explicit tools_read/tools_write entry below wins; else the
# upstream server's readOnlyHint annotation; else the tool counts as
# writing. Most servers don't set readOnlyHint, so access = "read" alone
# often exposes nothing — tools_read is how you classify a server from
# this side when the upstream hint is missing (symbrain doctor flags a
# read-access foreign server with no tools_read override). Replace "docs"
# and its command with the real foreign server you want to expose.
[servers.docs]
enabled    = true
command    = "/usr/local/bin/docs-mcp"   # or: url = "https://..."
access     = "read"
tools_read = ["search", "get"]           # tools you know are read-only

[audit]
enabled = true
`

func cmdInit(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	fs := flag.NewFlagSet("init", flag.ContinueOnError)
	fs.SetOutput(stderr)
	if err := fs.Parse(normalizeFlags(args)); err != nil {
		return exitcodes.ExitNoInput
	}

	dataDir, err := xdg.DataDir()
	if err != nil {
		fmt.Fprintf(stderr, "symbrain init: %v\n", err)
		return exitcodes.ExitGeneric
	}
	auditDir, err := xdg.AuditDir()
	if err != nil {
		fmt.Fprintf(stderr, "symbrain init: %v\n", err)
		return exitcodes.ExitGeneric
	}
	cacheDir, err := xdg.CacheDir()
	if err != nil {
		fmt.Fprintf(stderr, "symbrain init: %v\n", err)
		return exitcodes.ExitGeneric
	}

	dirs := []string{xdg.ConfigDir(), xdg.ProfilesDir(), dataDir, auditDir, cacheDir}
	for _, dir := range dirs {
		if err := os.MkdirAll(dir, 0o700); err != nil {
			fmt.Fprintf(stderr, "symbrain init: create %s: %v\n", dir, err)
			return exitcodes.ExitGeneric
		}
	}

	type file struct {
		path     string
		contents string
	}
	files := []file{
		{xdg.ConfigPath(), defaultConfigTOML},
		{filepath.Join(xdg.ProfilesDir(), "personal.toml"), personalProfileTOML},
		{filepath.Join(xdg.ProfilesDir(), "restricted.toml"), restrictedProfileTOML},
		{filepath.Join(xdg.ProfilesDir(), "foreign-read-only.toml"), foreignReadOnlyProfileTOML},
	}

	for _, f := range files {
		created, err := writeIfMissing(f.path, f.contents)
		if err != nil {
			fmt.Fprintf(stderr, "symbrain init: %s: %v\n", f.path, err)
			return exitcodes.ExitGeneric
		}
		if created {
			fmt.Fprintf(stdout, "created %s\n", f.path)
		} else {
			fmt.Fprintf(stdout, "skipped %s (already exists)\n", f.path)
		}
	}

	return exitcodes.ExitOK
}

// writeIfMissing atomically writes contents to path unless a file already
// exists there, so a re-run of `symbrain init` never overwrites user
// edits. It reports whether it created the file.
func writeIfMissing(path, contents string) (bool, error) {
	if _, err := os.Stat(path); err == nil {
		return false, nil
	} else if !os.IsNotExist(err) {
		return false, err
	}
	if err := fsutil.AtomicWriteFile(path, []byte(contents), 0o600); err != nil {
		return false, err
	}
	return true, nil
}
