package main

import (
	"flag"
	"fmt"
	"io"
	"os"
	"regexp"
	"strconv"

	"github.com/danieljustus/symaira-brain/internal/profile"
	"github.com/danieljustus/symaira-brain/internal/xdg"
	"github.com/danieljustus/symaira-corekit/exitcodes"
	"github.com/danieljustus/symaira-corekit/fsutil"
)

// profileNameFieldPattern matches the [profile] table's `name = "..."`
// line in the personal/restricted templates from cmd_init.go, capturing
// the whitespace between "name" and the value so the rewritten line keeps
// the template's own formatting.
var profileNameFieldPattern = regexp.MustCompile(`(?m)^name(\s*=\s*)"([^"]*)"`)

// renderProfileFromTemplate returns the personal or restricted profile
// template from cmd_init.go with its [profile] name field rewritten to
// name. It reuses those consts directly (same package) rather than
// forking a second copy of the TOML content.
func renderProfileFromTemplate(from, name string) (string, error) {
	var tmpl string
	switch from {
	case "personal":
		tmpl = personalProfileTOML
	case "restricted":
		tmpl = restrictedProfileTOML
	default:
		return "", fmt.Errorf("--from must be %q or %q, got %q", "personal", "restricted", from)
	}

	loc := profileNameFieldPattern.FindStringSubmatchIndex(tmpl)
	if loc == nil {
		return "", fmt.Errorf("internal error: %q template has no [profile] name field to rewrite", from)
	}
	ws := tmpl[loc[2]:loc[3]]
	return tmpl[:loc[0]] + "name" + ws + strconv.Quote(name) + tmpl[loc[1]:], nil
}

func cmdProfileAdd(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	args = reorderFlagsFirst(args, map[string]bool{"from": true})

	fs := flag.NewFlagSet("profile add", flag.ContinueOnError)
	from := fs.String("from", "restricted", `template to create from: "personal" or "restricted"`)
	fs.SetOutput(stderr)
	if err := fs.Parse(normalizeFlags(args)); err != nil {
		return exitcodes.ExitNoInput
	}
	if fs.NArg() != 1 {
		fmt.Fprintln(stderr, "usage: symbrain profile add <name> [--from personal|restricted]")
		return exitcodes.ExitNoInput
	}
	name := fs.Arg(0)

	if err := profile.ValidateName(name); err != nil {
		fmt.Fprintf(stderr, "symbrain profile add: %v\n", err)
		return exitcodes.ExitNoInput
	}
	if profile.Exists(name) {
		fmt.Fprintf(stderr, "symbrain profile add: profile %q already exists (%s)\n", name, profile.Path(name))
		return exitcodes.ExitNoInput
	}

	contents, err := renderProfileFromTemplate(*from, name)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain profile add: %v\n", err)
		return exitcodes.ExitNoInput
	}

	if err := os.MkdirAll(xdg.ProfilesDir(), 0o700); err != nil {
		fmt.Fprintf(stderr, "symbrain profile add: %v\n", err)
		return exitcodes.ExitGeneric
	}
	if err := fsutil.AtomicWriteFile(profile.Path(name), []byte(contents), 0o600); err != nil {
		fmt.Fprintf(stderr, "symbrain profile add: %v\n", err)
		return exitcodes.ExitGeneric
	}

	fmt.Fprintf(stdout, "created %s (from %s)\n", profile.Path(name), *from)
	return exitcodes.ExitOK
}
