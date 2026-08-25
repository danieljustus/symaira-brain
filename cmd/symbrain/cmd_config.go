package main

import (
	"bytes"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strconv"
	"strings"

	"github.com/BurntSushi/toml"
	"github.com/danieljustus/symaira-brain/internal/config"
	"github.com/danieljustus/symaira-corekit/configkit"
	"github.com/danieljustus/symaira-corekit/exitcodes"
	"github.com/danieljustus/symaira-corekit/fsutil"
)

// cmdConfig exposes the global (non-profile) configuration file:
// ~/.config/symbrain/config.toml. Profiles are a deliberate exception to
// the CLI vocabulary's <tool> config surface — they have their own
// dedicated commands (symbrain profile ...). `config init` is covered by
// `symbrain init`, which creates the file together with the XDG
// directories and example profiles.
func cmdConfig(args []string, stdout, stderr io.Writer) exitcodes.ExitCode {
	fs := flag.NewFlagSet("config", flag.ContinueOnError)
	fs.SetOutput(stderr)
	fs.Usage = func() { printConfigUsage(stderr) }
	if err := fs.Parse(args); err != nil {
		return exitcodes.ExitNoInput
	}
	if fs.NArg() < 1 {
		printConfigUsage(stderr)
		return exitcodes.ExitNoInput
	}

	switch sub := fs.Arg(0); sub {
	case "path":
		return configPath(stdout, fs.Args()[1:])
	case "get":
		return configGet(stdout, stderr, fs.Args()[1:])
	case "set":
		return configSet(stdout, stderr, fs.Args()[1:])
	default:
		fmt.Fprintf(stderr, "symbrain config: unknown subcommand %q (want path, get, or set)\n", sub)
		return exitcodes.ExitNoInput
	}
}

func printConfigUsage(w io.Writer) {
	fmt.Fprint(w, `symbrain config — inspect and edit the global config file

Usage:
  symbrain config path                 Print the global config file path
  symbrain config get [key]            Print the stored value for a dotted key
                                       (no key: print the whole file)
  symbrain config set <key> <value>    Set a dotted key; values are typed as
                                       bool, integer, or string

Keys live under ~/.config/symbrain/config.toml, e.g. default_profile,
audit.enabled, audit.verbose, gateway.identity_injection,
updatecheck.enabled, patterns.enabled, patterns.promotion_threshold,
servers.vault.binary_path.

Note: get/set operate on the global file as stored. The config loader also
merges a project-local .symbrain.toml and SYMBRAIN_* environment overrides
on top when commands run. Creating the file is 'symbrain init'; profiles
have their own commands ('symbrain profile ...').
`)
}

func configPath(stdout io.Writer, args []string) exitcodes.ExitCode {
	if len(args) > 0 {
		fmt.Fprintf(stdout, "symbrain config path: unexpected argument %q\n", args[0])
		return exitcodes.ExitNoInput
	}
	fmt.Fprintln(stdout, configkit.DefaultPath(config.AppName))
	return exitcodes.ExitOK
}

func configGet(stdout, stderr io.Writer, args []string) exitcodes.ExitCode {
	path := configkit.DefaultPath(config.AppName)

	if len(args) == 0 {
		raw, err := os.ReadFile(path)
		if err != nil {
			if os.IsNotExist(err) {
				fmt.Fprintf(stdout, "(no config file at %s)\n", path)
				return exitcodes.ExitOK
			}
			fmt.Fprintf(stderr, "symbrain config get: read %s: %v\n", path, err)
			return exitcodes.ExitGeneric
		}
		fmt.Fprint(stdout, string(raw))
		return exitcodes.ExitOK
	}
	if len(args) > 1 {
		fmt.Fprintf(stderr, "symbrain config get: unexpected argument %q\n", args[1])
		return exitcodes.ExitNoInput
	}

	root, err := readConfigMap(path)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain config get: %v\n", err)
		return exitcodes.ExitGeneric
	}
	value, ok := lookupKey(root, args[0])
	if !ok {
		fmt.Fprintf(stderr, "symbrain config get: key %q is not set in %s\n", args[0], path)
		return exitcodes.ExitNoInput
	}
	fmt.Fprintln(stdout, value)
	return exitcodes.ExitOK
}

func configSet(stdout, stderr io.Writer, args []string) exitcodes.ExitCode {
	if len(args) != 2 {
		fmt.Fprintln(stderr, "symbrain config set: want exactly <key> <value>")
		return exitcodes.ExitNoInput
	}
	key, value := args[0], args[1]
	if key == "" {
		fmt.Fprintln(stderr, "symbrain config set: key must not be empty")
		return exitcodes.ExitNoInput
	}

	path := configkit.DefaultPath(config.AppName)
	root, err := readConfigMap(path)
	if err != nil {
		fmt.Fprintf(stderr, "symbrain config set: %v\n", err)
		return exitcodes.ExitGeneric
	}
	if err := setKey(root, key, inferValue(value)); err != nil {
		fmt.Fprintf(stderr, "symbrain config set: %v\n", err)
		return exitcodes.ExitNoInput
	}

	var buf bytes.Buffer
	if err := toml.NewEncoder(&buf).Encode(root); err != nil {
		fmt.Fprintf(stderr, "symbrain config set: encode %s: %v\n", path, err)
		return exitcodes.ExitGeneric
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		fmt.Fprintf(stderr, "symbrain config set: create %s: %v\n", filepath.Dir(path), err)
		return exitcodes.ExitGeneric
	}
	if err := fsutil.AtomicWriteFile(path, buf.Bytes(), 0o600); err != nil {
		fmt.Fprintf(stderr, "symbrain config set: write %s: %v\n", path, err)
		return exitcodes.ExitGeneric
	}
	fmt.Fprintf(stdout, "set %s = %v in %s\n", key, value, path)
	return exitcodes.ExitOK
}

// readConfigMap decodes the global config file into a nested map, treating
// a missing file as an empty config.
func readConfigMap(path string) (map[string]any, error) {
	root := map[string]any{}
	if _, err := toml.DecodeFile(path, &root); err != nil {
		if os.IsNotExist(err) {
			return root, nil
		}
		return nil, fmt.Errorf("parse %s: %w", path, err)
	}
	return root, nil
}

// lookupKey walks a dotted key (e.g. "servers.vault.binary_path") through
// the nested map.
func lookupKey(root map[string]any, key string) (any, bool) {
	var current any = root
	for _, part := range strings.Split(key, ".") {
		m, ok := current.(map[string]any)
		if !ok {
			return nil, false
		}
		current, ok = m[part]
		if !ok {
			return nil, false
		}
	}
	return current, true
}

// setKey walks a dotted key, creating intermediate tables as needed, and
// stores value at the leaf.
func setKey(root map[string]any, key string, value any) error {
	parts := strings.Split(key, ".")
	current := root
	for _, part := range parts[:len(parts)-1] {
		next, ok := current[part]
		if !ok {
			child := map[string]any{}
			current[part] = child
			current = child
			continue
		}
		child, ok := next.(map[string]any)
		if !ok {
			return fmt.Errorf("cannot set %q: %q is not a table", key, part)
		}
		current = child
	}
	current[parts[len(parts)-1]] = value
	return nil
}

// inferValue types a CLI string as bool, integer, or string for storage.
func inferValue(s string) any {
	if b, err := strconv.ParseBool(s); err == nil {
		return b
	}
	if i, err := strconv.ParseInt(s, 10, 64); err == nil {
		return i
	}
	return s
}
