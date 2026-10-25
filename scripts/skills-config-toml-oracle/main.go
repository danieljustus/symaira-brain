// Command skills-config-toml-oracle freezes how the real Go symskills config
// loader decodes config.toml / .symskills.toml (#764, SKL-018).
//
// Two Go decoders read those files:
//
//   - config.Load: corekit configkit decodes the file into a generic
//     map[string]interface{} with BurntSushi/toml and copies the entries onto
//     config.Config by exact (case-sensitive) json tag.
//   - config.LoadTargets / config.LoadCapabilities: toml.DecodeFile straight
//     into a typed struct (case-insensitive field matching).
//
// Every corpus case writes a global and/or project file below one disposable
// root and records what the real functions return. The Rust decoder
// (rust/symbrain-skills/src/config_toml.rs) is replayed against the fixture by
// tests/config_toml_oracle.rs without a Go toolchain.
//
// Usage: go run ./scripts/skills-config-toml-oracle [-check]
package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"math/rand"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"unicode/utf8"

	"github.com/danieljustus/symaira-brain/internal/skills/config"
)

const fixturePath = "rust/symbrain-skills/tests/fixtures/config_toml_oracle.json"

// input is one config file body. Exactly one of UTF8, B64 or Repeat is set.
type input struct {
	UTF8   *string `json:"utf8,omitempty"`
	B64    *string `json:"b64,omitempty"`
	Repeat *repeat `json:"repeat,omitempty"`
}

// repeat is a compact spelling of huge inputs: Prefix + Unit*Count + Suffix.
type repeat struct {
	Prefix string `json:"prefix"`
	Unit   string `json:"unit"`
	Count  int    `json:"count"`
	Suffix string `json:"suffix"`
}

func (i *input) bytes() []byte {
	switch {
	case i == nil:
		return nil
	case i.UTF8 != nil:
		return []byte(*i.UTF8)
	case i.B64 != nil:
		b, err := base64.StdEncoding.DecodeString(*i.B64)
		if err != nil {
			panic(err)
		}
		return b
	default:
		return []byte(i.Repeat.Prefix + strings.Repeat(i.Repeat.Unit, i.Repeat.Count) + i.Repeat.Suffix)
	}
}

func text(s string) *input {
	if utf8.ValidString(s) {
		return &input{UTF8: &s}
	}
	b := base64.StdEncoding.EncodeToString([]byte(s))
	return &input{B64: &b}
}

type effective struct {
	LibraryDir  string `json:"library_dir"`
	RenderDir   string `json:"render_dir"`
	CacheDir    string `json:"cache_dir"`
	ProfilesDir string `json:"profiles_dir"`
	BaseDir     string `json:"base_dir"`
	VCSEnabled  bool   `json:"vcs_enabled"`
}

// loadResult records config.Load. Config holds only the fields that differ
// from the recorded defaults, which keeps the fixture small; defaults plus
// delta is the effective config.
type loadResult struct {
	Error  string         `json:"error,omitempty"`
	Config map[string]any `json:"config,omitempty"`
}

func delta(defaults, got effective) map[string]any {
	out := map[string]any{}
	if got.LibraryDir != defaults.LibraryDir {
		out["library_dir"] = got.LibraryDir
	}
	if got.RenderDir != defaults.RenderDir {
		out["render_dir"] = got.RenderDir
	}
	if got.CacheDir != defaults.CacheDir {
		out["cache_dir"] = got.CacheDir
	}
	if got.ProfilesDir != defaults.ProfilesDir {
		out["profiles_dir"] = got.ProfilesDir
	}
	if got.BaseDir != defaults.BaseDir {
		out["base_dir"] = got.BaseDir
	}
	if got.VCSEnabled != defaults.VCSEnabled {
		out["vcs_enabled"] = got.VCSEnabled
	}
	return out
}

// typedRuns is how often each typed decode is repeated: BurntSushi iterates a
// Go map while unifying a struct, so a document with two keys on one field, or
// two failing fields, is decided by random map order. Such a case is recorded
// as Nondeterministic (no expected value) and must be left to Go.
const typedRuns = 40

type targetsResult struct {
	Nondeterministic bool                       `json:"nondeterministic,omitempty"`
	Error            string                     `json:"error,omitempty"`
	Targets          []config.CustomTarget      `json:"targets"`
	Caps             map[string]map[string]bool `json:"capabilities"`
}

type caseFixture struct {
	ID      string        `json:"id"`
	Group   string        `json:"group"`
	Global  *input        `json:"global"`
	Project *input        `json:"project"`
	Load    loadResult    `json:"load"`
	Targets targetsResult `json:"targets_decode"`
}

type provenance struct {
	Revision     string `json:"revision"`
	Command      string `json:"command"`
	Toolchain    string `json:"toolchain"`
	TOMLLibrary  string `json:"toml_library"`
	Corekit      string `json:"corekit"`
	ConfigSource string `json:"config_go_sha256"`
	Note         string `json:"note"`
}

type fixture struct {
	Provenance  provenance    `json:"provenance"`
	Defaults    effective     `json:"defaults"`
	GlobalPath  string        `json:"global_path"`
	ProjectPath string        `json:"project_path"`
	Cases       []caseFixture `json:"cases"`
}

type corpusCase struct {
	id, group       string
	global, project *input
}

func main() {
	check := flag.Bool("check", false, "fail when the committed fixture differs from the real Go decoders")
	out := flag.String("out", fixturePath, "fixture path to write (soak runs use a scratch path)")
	flag.IntVar(&mutationCount, "mutations", mutationCount, "number of seeded mutation cases")
	flag.Int64Var(&mutationSeed, "seed", mutationSeed, "mutation corpus seed")
	flag.Parse()

	current, err := generate()
	if err != nil {
		fmt.Fprintf(os.Stderr, "generate: %v\n", err)
		os.Exit(1)
	}
	if *check {
		data, err := os.ReadFile(fixturePath)
		if err != nil {
			fmt.Fprintf(os.Stderr, "read fixture: %v\n", err)
			os.Exit(1)
		}
		var stored fixture
		if err := json.Unmarshal(data, &stored); err != nil {
			fmt.Fprintf(os.Stderr, "parse fixture: %v\n", err)
			os.Exit(1)
		}
		if !sameFixture(stored, current) {
			fmt.Fprintln(os.Stderr, "fixture drift detected (Go decoder behavior changed)")
			os.Exit(1)
		}
		fmt.Printf("fixture matches Go (check passed, %d cases)\n", len(current.Cases))
		return
	}
	data, err := marshalFixture(current)
	if err != nil {
		fmt.Fprintf(os.Stderr, "marshal: %v\n", err)
		os.Exit(1)
	}
	if err := os.MkdirAll(filepath.Dir(*out), 0o755); err != nil {
		fmt.Fprintf(os.Stderr, "mkdir: %v\n", err)
		os.Exit(1)
	}
	if err := os.WriteFile(*out, data, 0o644); err != nil {
		fmt.Fprintf(os.Stderr, "write: %v\n", err)
		os.Exit(1)
	}
	fmt.Printf("fixture written to %s (%d cases)\n", *out, len(current.Cases))
}

func marshalJSON(v any) ([]byte, error) {
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	enc.SetEscapeHTML(false)
	if err := enc.Encode(v); err != nil {
		return nil, err
	}
	return bytes.TrimRight(buf.Bytes(), "\n"), nil
}

// marshalFixture writes one case per line so a regenerated fixture diffs case
// by case.
func marshalFixture(fx fixture) ([]byte, error) {
	head := fx
	head.Cases = nil
	headJSON, err := marshalJSON(head)
	if err != nil {
		return nil, err
	}
	var out bytes.Buffer
	out.Write(bytes.TrimSuffix(headJSON, []byte(`,"cases":null}`)))
	out.WriteString(",\"cases\":[\n")
	for i, c := range fx.Cases {
		cj, err := marshalJSON(c)
		if err != nil {
			return nil, err
		}
		out.Write(cj)
		if i+1 < len(fx.Cases) {
			out.WriteByte(',')
		}
		out.WriteByte('\n')
	}
	out.WriteString("]}\n")
	return out.Bytes(), nil
}

// sameFixture compares everything except the provenance (the recorded
// revision does not survive a squash merge).
func sameFixture(a, b fixture) bool {
	a.Provenance, b.Provenance = provenance{}, provenance{}
	aj, _ := json.Marshal(a)
	bj, _ := json.Marshal(b)
	return bytes.Equal(aj, bj)
}

func generate() (fixture, error) {
	orig, err := os.Getwd()
	if err != nil {
		return fixture{}, err
	}
	defer os.Chdir(orig)
	prov := provenanceNow()
	base, err := os.MkdirTemp("", "symskills-config-toml-oracle-")
	if err != nil {
		return fixture{}, err
	}
	defer os.RemoveAll(base)
	root, err := filepath.EvalSymlinks(base)
	if err != nil {
		return fixture{}, err
	}
	home := filepath.Join(root, "home")
	proj := filepath.Join(root, "proj")
	cfgDir := filepath.Join(home, ".config", "symskills")
	for _, d := range []string{cfgDir, proj} {
		if err := os.MkdirAll(d, 0o755); err != nil {
			return fixture{}, err
		}
	}
	globalPath := filepath.Join(cfgDir, "config.toml")
	projectPath := filepath.Join(proj, ".symskills.toml")

	for _, key := range os.Environ() {
		name := strings.SplitN(key, "=", 2)[0]
		if strings.HasPrefix(name, "SYMSKILLS_") || strings.HasPrefix(name, "SYMBRAIN_") || strings.HasPrefix(name, "XDG_") {
			os.Unsetenv(name)
		}
	}
	os.Setenv("HOME", home)
	if err := os.Chdir(proj); err != nil {
		return fixture{}, err
	}

	norm := func(s string) string { return strings.ReplaceAll(s, root, "$ROOT") }
	fx := fixture{
		Provenance:  prov,
		GlobalPath:  norm(globalPath),
		ProjectPath: norm(projectPath),
	}
	d := config.Defaults()
	fx.Defaults = effective{norm(d.LibraryDir), norm(d.RenderDir), norm(d.CacheDir), norm(d.ProfilesDir), norm(d.BaseDir), d.VCSEnabled()}

	for _, c := range corpus() {
		os.Remove(globalPath)
		os.Remove(projectPath)
		if c.global != nil {
			if err := os.WriteFile(globalPath, c.global.bytes(), 0o644); err != nil {
				return fixture{}, err
			}
		}
		if c.project != nil {
			if err := os.WriteFile(projectPath, c.project.bytes(), 0o644); err != nil {
				return fixture{}, err
			}
		}
		cf := caseFixture{ID: c.id, Group: c.group, Global: c.global, Project: c.project}
		cfg, err := config.Load()
		if err != nil {
			cf.Load.Error = norm(err.Error())
		} else {
			cf.Load.Config = delta(fx.Defaults, effective{norm(cfg.LibraryDir), norm(cfg.RenderDir), norm(cfg.CacheDir), norm(cfg.ProfilesDir), norm(cfg.BaseDir), cfg.VCSEnabled()})
		}
		cf.Targets = typedDecode()
		for i := 1; i < typedRuns && !cf.Targets.Nondeterministic; i++ {
			again := typedDecode()
			a, _ := json.Marshal(norm2(cf.Targets, norm))
			b, _ := json.Marshal(norm2(again, norm))
			if !bytes.Equal(a, b) {
				cf.Targets = targetsResult{Nondeterministic: true}
			}
		}
		cf.Targets = norm2(cf.Targets, norm)
		fx.Cases = append(fx.Cases, cf)
	}
	return fx, nil
}

func typedDecode() targetsResult {
	var out targetsResult
	targets, terr := config.LoadTargets()
	caps, cerr := config.LoadCapabilities()
	switch {
	case terr != nil:
		out.Error = terr.Error()
	case cerr != nil:
		out.Error = cerr.Error()
	default:
		out.Targets = targets
		out.Caps = caps
	}
	return out
}

func norm2(r targetsResult, norm func(string) string) targetsResult {
	r.Error = norm(r.Error)
	return r
}

func provenanceNow() provenance {
	src, _ := os.ReadFile("internal/skills/config/config.go")
	sum := sha256.Sum256(src)
	return provenance{
		Revision:     gitOutput("rev-parse", "HEAD"),
		Command:      "go run ./scripts/skills-config-toml-oracle",
		Toolchain:    goToolchain(),
		TOMLLibrary:  goModVersion("github.com/BurntSushi/toml"),
		Corekit:      goModVersion("github.com/danieljustus/symaira-corekit"),
		ConfigSource: hex.EncodeToString(sum[:]),
		Note: "Captured from this working tree. A squash merge destroys the recorded revision; " +
			"scripts/run-go-oracle.sh -check compares every case and ignores provenance.",
	}
}

func gitOutput(args ...string) string {
	out, err := exec.Command("git", args...).Output()
	if err != nil {
		return "unknown"
	}
	return strings.TrimSpace(string(out))
}

func goModLines() []string {
	data, err := os.ReadFile("go.mod")
	if err != nil {
		return nil
	}
	return strings.Split(string(data), "\n")
}

func goToolchain() string {
	for _, line := range goModLines() {
		f := strings.Fields(line)
		if len(f) == 2 && f[0] == "go" {
			return "go" + f[1]
		}
	}
	return "unknown"
}

func goModVersion(module string) string {
	for _, line := range goModLines() {
		f := strings.Fields(line)
		if len(f) >= 2 && f[0] == module {
			return f[1]
		}
		if len(f) >= 3 && f[0] == "require" && f[1] == module {
			return f[2]
		}
	}
	return "unknown"
}

// ---------------------------------------------------------------- corpus --

func corpus() []corpusCase {
	var out []corpusCase
	add := func(group, id string, global, project *input) {
		out = append(out, corpusCase{id: group + "/" + id, group: group, global: global, project: project})
	}
	g := func(group, id, body string) { add(group, id, text(body), nil) }

	g("empty", "empty-file", "")
	g("empty", "only-newline", "\n")
	g("empty", "only-comment", "# nothing here\n")
	g("empty", "only-whitespace", "  \t \n\t\n")
	g("empty", "no-trailing-newline", `library_dir = "/a"`)
	g("empty", "bom-only", "\xef\xbb\xbf")
	g("empty", "bom-valid", "\xef\xbb\xbflibrary_dir = \"/a\"\n")
	g("empty", "bom-twice", "\xef\xbb\xbf\xef\xbb\xbflibrary_dir = \"/a\"\n")
	g("empty", "bom-mid", "library_dir = \"/a\"\n\xef\xbb\xbfrender_dir = \"/b\"\n")
	g("empty", "bom-crlf", "\xef\xbb\xbflibrary_dir = \"/a\"\r\nrender_dir = \"/b\"\r\n")
	g("empty", "nul-byte", "library_dir = \"/a\"\x00\n")
	g("empty", "nul-only", "\x00")
	g("empty", "invalid-utf8-in-string", "library_dir = \"/a\xff\"\n")
	g("empty", "invalid-utf8-in-comment", "# \xff\xfe\nlibrary_dir = \"/a\"\n")
	g("empty", "invalid-utf8-in-key", "libr\xffary = 1\n")
	g("empty", "invalid-utf8-truncated", "library_dir = \"/a\xe2\x82\"\n")
	g("empty", "overlong-utf8", "library_dir = \"\xc0\xaf\"\n")
	g("empty", "surrogate-utf8", "library_dir = \"\xed\xa0\x80\"\n")

	fields := []string{"library_dir", "render_dir", "cache_dir", "profiles_dir", "base_dir"}
	// All five string fields plus vcs, every spelling of string.
	g("valid", "all-fields", "library_dir = \"/l\"\nrender_dir = \"/r\"\ncache_dir = \"/c\"\nprofiles_dir = \"/p\"\nbase_dir = \"/b\"\n\n[vcs]\nenabled = false\n")
	g("valid", "vcs-true", "[vcs]\nenabled = true\n")
	g("valid", "vcs-false", "[vcs]\nenabled = false\n")
	g("valid", "vcs-dotted", "vcs.enabled = false\n")
	g("valid", "vcs-inline", "vcs = { enabled = false }\n")
	g("valid", "vcs-inline-true", "vcs = { enabled = true }\n")
	g("valid", "vcs-empty-table", "[vcs]\n")
	g("valid", "vcs-empty-inline", "vcs = {}\n")
	g("valid", "vcs-quoted-key", "[\"vcs\"]\n\"enabled\" = false\n")
	g("valid", "vcs-literal-key", "['vcs']\n'enabled' = false\n")
	g("valid", "vcs-spaced-header", "[ vcs ]\nenabled = false\n")
	g("valid", "vcs-dotted-header", "[ vcs . x ]\n[vcs]\nenabled = false\n")
	g("valid", "vcs-unknown-sibling", "[vcs]\nenabled = false\nfoo = 1\n")
	g("valid", "vcs-nested-unknown", "[vcs]\nenabled = false\n[vcs.sub]\nx = 1\n")
	g("valid", "vcs-string-true", "[vcs]\nenabled = \"true\"\n")
	for _, s := range []string{"1", "t", "T", "TRUE", "true", "True", "0", "f", "F", "FALSE", "false", "False", "yes", "no", "tRUE", "", " true", "on", "off", "2"} {
		g("vcs-bool-string", fmt.Sprintf("%q", s), fmt.Sprintf("[vcs]\nenabled = %q\n", s))
	}
	for _, v := range []string{"1", "0", "1.5", "[true]", "{ a = true }", "1979-05-27", "07:32:00", "1979-05-27T07:32:00", "1979-05-27T07:32:00Z", "1979-05-27T07:32:00+01:00", "[]", "inf", "nan", "0x1", `'t'`, `"""t"""`, `'''true'''`} {
		g("vcs-bool-other", v, "[vcs]\nenabled = "+v+"\n")
	}
	g("vcs-shape", "vcs-string", "vcs = \"x\"\n")
	g("vcs-shape", "vcs-int", "vcs = 3\n")
	g("vcs-shape", "vcs-bool", "vcs = true\n")
	g("vcs-shape", "vcs-array", "vcs = [1, 2]\n")
	g("vcs-shape", "vcs-array-of-tables", "[[vcs]]\nenabled = false\n")
	g("vcs-shape", "vcs-array-of-inline", "vcs = [{ enabled = false }]\n")
	g("vcs-shape", "vcs-then-bad-type", "vcs = 3\nlibrary_dir = 1\n")
	g("vcs-shape", "enabled-top-level", "enabled = false\n")
	g("vcs-shape", "vcs-upper", "[VCS]\nenabled = false\n")
	g("vcs-shape", "enabled-upper", "[vcs]\nEnabled = false\n")
	g("vcs-shape", "enabled-array", "[vcs]\nenabled = [false]\n")
	g("vcs-shape", "enabled-array-empty", "[vcs]\nenabled = []\n")

	for _, f := range fields {
		g("string-ok", f+"-basic", f+" = \"/some/where\"\n")
		g("string-ok", f+"-empty", f+" = \"\"\n")
		g("string-ok", f+"-literal", f+" = '/some/where'\n")
		g("string-ok", f+"-literal-backslash", f+" = 'C:\\Users\\x'\n")
		g("string-ok", f+"-ml-basic", f+" = \"\"\"\n/a\n/b\"\"\"\n")
		g("string-ok", f+"-ml-literal", f+" = '''\n/a\n/b'''\n")
		g("string-ok", f+"-ml-line-continue", f+" = \"\"\"/a \\\n    /b\"\"\"\n")
		g("string-ok", f+"-unicode", f+" = \"/ünï/日本/😀\"\n")
		g("string-ok", f+"-escapes", f+" = \"a\\tb\\nc\\\\d\\\"e\\u00e9\\U0001F600\\b\\f\\r\"\n")
		g("string-ok", f+"-escape-e", f+" = \"a\\eb\"\n")
		g("string-ok", f+"-escape-x", f+" = \"a\\x41b\"\n")
		g("string-ok", f+"-quoted-key", "\""+f+"\" = \"/q\"\n")
		g("string-ok", f+"-literal-key", "'"+f+"' = \"/q\"\n")
		g("string-ok", f+"-spaced", f+"   =   \"/s\"   \n")
		g("string-ok", f+"-comment", f+" = \"/s\" # trailing\n")
		g("string-ok", f+"-hash-in-string", f+" = \"/s#x\" # trailing\n")
		g("string-ok", f+"-crlf", f+" = \"/s\"\r\n")
		g("string-ok", f+"-dotted-split", f+" = \"/s\"\n[other]\nx = 1\n")
		g("string-ok", f+"-after-table-is-nested", "[t]\n"+f+" = \"/s\"\n")
		g("string-ok", f+"-in-inline-root-table", "x = { "+f+" = \"/s\" }\n")
		g("string-ok", f+"-escaped-key", "\""+strings.Replace(f, "_", "\\u005f", 1)+"\" = \"/e\"\n")
		g("string-ok", f+"-upper-key", strings.ToUpper(f)+" = \"/u\"\n")
		g("string-ok", f+"-camel-key", strings.ReplaceAll(strings.Title(strings.ReplaceAll(f, "_", " ")), " ", "")+" = \"/u\"\n")
		g("string-ok", f+"-dash-key", strings.ReplaceAll(f, "_", "-")+" = \"/u\"\n")
		g("string-ok", f+"-dotted-subkey", f+".x = \"/u\"\n")
		g("string-ok", f+"-trailing-space-key", f+" \t= \"/s\"\n")
		for _, v := range []struct{ id, lit string }{
			{"int", "1"}, {"neg-int", "-5"}, {"float", "1.5"}, {"bool", "true"}, {"bool-false", "false"},
			{"array", "[\"/a\"]"}, {"array-empty", "[]"}, {"array-mixed", "[1, \"a\"]"}, {"inline-table", "{ a = 1 }"}, {"inline-empty", "{}"},
			{"date", "1979-05-27"}, {"time", "07:32:00"}, {"local-datetime", "1979-05-27T07:32:00"},
			{"offset-datetime", "1979-05-27T07:32:00Z"}, {"offset-datetime-frac", "1979-05-27T00:32:00.999999-07:00"},
			{"array-inline-tables", "[{ a = 1 }, { b = 2 }]"}, {"array-of-arrays", "[[1], [2]]"}, {"hex", "0xDEAD_beef"}, {"oct", "0o755"}, {"bin", "0b101"}, {"inf", "inf"}, {"nan", "nan"}, {"exp", "1e6"},
			{"underscore-int", "1_000"}, {"plus-int", "+7"}, {"time-noseconds", "07:32"}, {"datetime-space", "1979-05-27 07:32:00Z"},
		} {
			g("string-wrong-type", f+"-"+v.id, f+" = "+v.lit+"\n")
		}
		g("string-wrong-type", f+"-table", "["+f+"]\nx = 1\n")
		g("string-wrong-type", f+"-array-of-tables", "[["+f+"]]\nx = 1\n")
		g("string-wrong-type", f+"-table-after-valid", f+" = \"/a\"\n["+f+"]\n")
		g("duplicate", f+"-twice", f+" = \"/a\"\n"+f+" = \"/b\"\n")
		g("duplicate", f+"-twice-same", f+" = \"/a\"\n"+f+" = \"/a\"\n")
		g("duplicate", f+"-bare-vs-quoted", f+" = \"/a\"\n\""+f+"\" = \"/b\"\n")
		g("duplicate", f+"-case-twins", f+" = \"/a\"\n"+strings.ToUpper(f)+" = \"/b\"\n")
		g("duplicate", f+"-then-table", f+" = \"/a\"\n["+f+"]\n")
		g("duplicate", f+"-dotted-then-value", f+".x = 1\n"+f+" = \"/a\"\n")
	}
	g("string-ok", "two-fields-order", "base_dir = \"/b\"\nlibrary_dir = \"/l\"\n")
	g("string-ok", "unknown-keys", "foo = 1\nbar = \"x\"\n[baz]\nqux = [1, 2]\nlibrary_dir = \"/in-table\"\n")
	g("string-ok", "nested-table-library", "[x]\nlibrary_dir = \"/in-x\"\n")
	g("string-ok", "dotted-nesting", "a.b.c = 1\na.b.d = 2\nlibrary_dir = \"/l\"\n")
	g("string-ok", "table-before-key-nests", "[t]\nx = 1\nlibrary_dir = \"/nested\"\n")
	g("string-ok", "string-all-escapes-invalid", "library_dir = \"\\q\"\n")
	g("string-ok", "surrogate-escape", "library_dir = \"\\ud800\"\n")
	g("string-ok", "out-of-range-escape", "library_dir = \"\\U00110000\"\n")
	g("string-ok", "nul-escape", "library_dir = \"a\\u0000b\"\n")
	g("string-ok", "del-char", "library_dir = \"a\x7fb\"\n")
	g("string-ok", "tab-char", "library_dir = \"a\tb\"\n")
	g("string-ok", "control-char", "library_dir = \"a\x01b\"\n")
	g("string-ok", "raw-newline-in-basic", "library_dir = \"a\nb\"\n")
	g("string-ok", "raw-cr-in-basic", "library_dir = \"a\rb\"\n")
	g("string-ok", "bare-cr-line-end", "library_dir = \"a\"\rrender_dir = \"b\"\n")
	g("string-ok", "crlf-ml", "library_dir = \"\"\"\r\na\r\nb\"\"\"\r\n")
	g("string-ok", "ml-quotes", "library_dir = \"\"\"a\"\"b\"\"\"\"\n")
	g("string-ok", "ml-five-quotes", "library_dir = \"\"\"a\"\"\"\"\"\n")
	g("string-ok", "ml-six-quotes", "library_dir = \"\"\"a\"\"\"\"\"\"\n")
	g("string-ok", "ml-literal-quotes", "library_dir = '''a''b''''\n")
	g("string-ok", "ml-leading-newline-trim", "library_dir = \"\"\"\n\nx\"\"\"\n")
	g("string-ok", "ml-crlf-leading", "library_dir = \"\"\"\r\nx\"\"\"\n")
	g("string-ok", "ml-continue-eof", "library_dir = \"\"\"a\\")
	g("string-ok", "ml-continue-spaces", "library_dir = \"\"\"a \\   \n\n   b\"\"\"\n")
	g("string-ok", "ml-continue-invalid", "library_dir = \"\"\"a \\ x\n\"\"\"\n")
	g("string-ok", "unterminated-basic", "library_dir = \"abc\n")
	g("string-ok", "unterminated-ml", "library_dir = \"\"\"abc\n")
	g("string-ok", "unterminated-literal", "library_dir = 'abc\n")
	g("string-ok", "line-sep-chars", "library_dir = \"a\u2028b\u2029c\"\n")
	g("string-ok", "string-with-bom-inside", "library_dir = \"a\ufeffb\"\n")
	g("string-ok", "emoji-key-unknown", "\"😀\" = 1\nlibrary_dir = \"/e\"\n")
	g("string-ok", "empty-key", "\"\" = 1\nlibrary_dir = \"/e\"\n")
	g("string-ok", "empty-key-bare", " = 1\n")
	g("string-ok", "key-only", "library_dir\n")
	g("string-ok", "key-no-value", "library_dir =\n")
	g("string-ok", "value-no-key", "= \"x\"\n")
	g("string-ok", "two-values", "library_dir = \"a\" \"b\"\n")
	g("string-ok", "two-per-line", "library_dir = \"a\" render_dir = \"b\"\n")
	g("string-ok", "single-quote-start", "library_dir = '\n")
	g("string-ok", "eq-eq", "library_dir == \"x\"\n")
	g("string-ok", "colon-instead", "library_dir: \"x\"\n")
	g("string-ok", "unquoted-value", "library_dir = /tmp/x\n")
	g("string-ok", "trailing-comma", "library_dir = \"x\",\n")

	// Tables, arrays of tables and structural conflicts.
	g("structure", "table-redefine", "[a]\nx = 1\n[a]\ny = 2\n")
	g("structure", "table-after-dotted", "a.b = 1\n[a]\nc = 2\n")
	g("structure", "table-super-after", "[a.b]\nx = 1\n[a]\ny = 2\n")
	g("structure", "table-dotted-extend", "[a.b]\nx = 1\n[a.b.c]\ny = 2\n")
	g("structure", "table-vs-value", "a = 1\n[a]\n")
	g("structure", "value-vs-table", "[a]\n[a.b]\n[a]\n")
	g("structure", "aot-basic", "[[a]]\nx = 1\n[[a]]\nx = 2\n")
	g("structure", "aot-then-table", "[[a]]\nx = 1\n[a]\ny = 2\n")
	g("structure", "table-then-aot", "[a]\nx = 1\n[[a]]\ny = 2\n")
	g("structure", "aot-sub-table", "[[a]]\n[a.b]\nx = 1\n[[a]]\n[a.b]\nx = 2\n")
	g("structure", "aot-sub-aot", "[[a]]\n[[a.b]]\n[[a]]\n[[a.b]]\n")
	g("structure", "aot-static-array", "a = [{x = 1}]\n[[a]]\n")
	g("structure", "static-array-extend", "a = [1]\n[[a]]\n")
	g("structure", "inline-table-extend", "a = { b = 1 }\n[a.c]\n")
	g("structure", "inline-table-extend-key", "a = { b = 1 }\na.c = 2\n")
	g("structure", "inline-table-dup-key", "a = { b = 1, b = 2 }\n")
	g("structure", "inline-table-dotted-conflict", "a = { b = 1, b.c = 2 }\n")
	g("structure", "inline-table-trailing-comma", "a = { b = 1, }\n")
	g("structure", "inline-table-newline", "a = { b = 1,\n c = 2 }\n")
	g("structure", "inline-table-newline-start", "a = {\n b = 1 }\n")
	g("structure", "inline-table-comment", "a = { b = 1, # c\n c = 2 }\n")
	g("structure", "inline-table-empty-comma", "a = { , }\n")
	g("structure", "inline-table-nested", "a = { b = { c = { d = 1 } } }\n")
	g("structure", "inline-table-unclosed", "a = { b = 1\n")
	g("structure", "array-multiline", "a = [\n 1,\n 2, # c\n]\n")
	g("structure", "array-mixed-types", "a = [1, \"a\", true, [1], { b = 1 }]\n")
	g("structure", "array-double-comma", "a = [1,,2]\n")
	g("structure", "array-leading-comma", "a = [,1]\n")
	g("structure", "array-unclosed", "a = [1, 2\n")
	g("structure", "array-nested-deep", "a = [[[[[[[[1]]]]]]]]\n")
	g("structure", "header-unclosed", "[a\nx = 1\n")
	g("structure", "header-empty", "[]\nx = 1\n")
	g("structure", "header-dot-empty", "[a.]\n")
	g("structure", "header-dot-lead", "[.a]\n")
	g("structure", "header-double-dot", "[a..b]\n")
	g("structure", "header-trailing-junk", "[a] x\n")
	g("structure", "header-comment", "[a] # c\nx = 1\n")
	g("structure", "header-aot-unclosed", "[[a]\n")
	g("structure", "header-aot-mismatch", "[[a]]]\n")
	g("structure", "header-space-inside", "[ [a] ]\n")
	g("structure", "header-quoted-dotted", "[\"a.b\"]\nx = 1\n")
	g("structure", "header-quoted-empty", "[\"\"]\nx = 1\n")
	g("structure", "header-same-line", "[a] x = 1\n")
	g("structure", "key-dotted-space", "a . b = 1\n")
	g("structure", "key-dotted-quoted", "a.\"b.c\".d = 1\n")
	g("structure", "key-bare-chars", "a-b_c1 = 1\n")
	g("structure", "key-numeric", "1234 = 1\n")
	g("structure", "key-float-like", "1.2 = 1\n")
	g("structure", "key-bad-char", "a$b = 1\n")
	g("structure", "key-unicode-bare", "é = 1\n")
	g("structure", "key-newline-in-quoted", "\"a\nb\" = 1\n")
	g("structure", "key-multiline-string", "\"\"\"a\"\"\" = 1\n")
	g("structure", "table-is-library-name", "[library_dir]\nx = 1\n")
	g("structure", "targets-table-no-effect", "[[targets]]\nname = \"x\"\nskill_root_user = \"/s\"\n")
	g("structure", "capabilities-table-no-effect", "[capabilities.codex]\nsubagents = true\n")

	// Numbers, dates in unknown positions (syntax acceptance corpus).
	for _, v := range []string{
		"0", "+0", "-0", "1_000", "1__0", "_1", "1_", "01", "00", "0x", "0xg", "0x_1", "0x1_", "0X1", "0b2", "0b", "0o8", "0o", "9223372036854775807", "9223372036854775808",
		"-9223372036854775808", "-9223372036854775809", "0xFFFFFFFFFFFFFFFF", "0x7FFFFFFFFFFFFFFF", "1.0", "1.", ".1", "1.e1", "1e", "1e+", "1E5", "1e+5", "1e-5", "1_0.0_1", "1._1", "1.1_", "+1.0", "-1.0", "-0.0",
		"inf", "+inf", "-inf", "nan", "+nan", "-nan", "Inf", "NaN", "infinity", "1e400", "1e-400", "0.0000001", "123456789012345678901234567890",
		"true", "false", "True", "TRUE", "tru", "truee", "yes",
		"1979-05-27", "1979-05-27T07:32:00Z", "1979-05-27T07:32:00z", "1979-05-27t07:32:00Z", "1979-05-27 07:32:00", "1979-05-27T07:32:00.123456789", "1979-05-27T07:32:00.1234567890123",
		"1979-05-27T07:32:00+07:30", "1979-05-27T07:32:00-00:00", "1979-05-27T07:32:00+24:00", "1979-05-27T07:32:00+00:60", "1979-05-27T25:00:00", "1979-05-27T07:60:00", "1979-05-27T07:32:60", "1979-05-27T07:32:61",
		"1979-13-27", "1979-00-27", "1979-02-30", "1979-02-29", "1980-02-29", "1900-02-29", "2000-02-29", "0000-01-01", "9999-12-31", "10000-01-01", "79-05-27", "1979-5-27", "1979-05-7", "1979-05-27T", "1979-05-27T07", "1979-05-27T07:32", "1979-05-27 ",
		"07:32:00", "07:32:00.5", "07:32", "7:32:00", "24:00:00", "23:59:60", "00:00:00.000000000", "1979-05-27T07:32Z", "1979-05-27T07:32:00 Z",
		"[1, 2,]", "[ ]", "[\n]", "{}", "{ a = 1 }", "{ a = 1, }", "[ { a = 1 }, { b = 2 } ]", "'a'", "\"a\"", "\"\"\"a\"\"\"", "'''a'''", "\"\\u00zz\"", "\"\\U0000zzzz\"", "\"\\uD7FF\"", "\"\\uE000\"", "\"\\u10FFFF\"",
		"\"\\x\"", "\"\\x4\"", "\"\\xZZ\"", "\"\\e\"", "\"\\a\"", "\"\\v\"", "\"\\ \"", "\"\\/\"", "\"\\'\"",
	} {
		g("syntax-values", v, "x = "+v+"\nlibrary_dir = \"/ok\"\n")
	}
	for _, v := range []string{
		"a = 1\r\nb = 2\r\n", "a = 1\r\n\r\nb = 2\r\n", "a = 1\r", "a = 1\n\r", "a = 1\r\r\nb = 2\n", "a = \"x\"\r\n[t]\r\nb = 2\r\n", "a = '''\r\nx\r\n'''\r\n", "a = [\r\n1,\r\n2\r\n]\r\n",
		"a = 1 # c\r\nb = 2\n", "# c\r\n", "# c\r", "# c \x01\n", "# c \x7f\n", "# c \x00\n", "# c\t\n", "a = 1 # \xff\n", "\n\n\na = 1\n\n\n", "\ta = 1\n", "  [t]\n  b = 1\n", "a\t=\t1\n", "a = 1\x0b\n", "a = 1\f\n", "a = 1\u00a0\n", "a\u00a0= 1\n", "a = 1\u3000\n",
		"a=1\nb=2", "a=1;b=2\n", "a=1,b=2\n", "[t]\n[t.u]\n[t]\n", "[[t]]\n[t.u]\n[[t]]\n[t.u]\n", "[a.b.c]\n[a]\n[a.b]\n", "[a]\n[a.b.c]\n[a.b]\n", "[a]\nb.c = 1\n[a.b]\n", "[a]\nb.c = 1\n[a.b.d]\n",
		"a.b.c = 1\na.b.d = 2\na.e = 3\n", "a.b = 1\na = 2\n", "a = 1\na.b = 2\n", "a = {}\na.b = 1\n", "[a]\n[a.b]\n[a.b]\n", "[x.y.z.w]\n[x]\nq=1\n", "[[a.b]]\n[a]\n", "[a]\n[[a.b]]\n[[a.b]]\n[a.b.c]\n",
	} {
		g("syntax-misc", fmt.Sprintf("%q", v), v)
	}

	// TOML 1.1 (BurntSushi v1.6.0 accepts) and TOML-1.0-only divergences.
	g("toml11", "inline-newline", "x = {\n  a = 1,\n  b = 2,\n}\nlibrary_dir = \"/ok\"\n")
	g("toml11", "time-no-seconds", "x = 07:32\nlibrary_dir = \"/ok\"\n")
	g("toml11", "datetime-no-seconds", "x = 1979-05-27T07:32Z\nlibrary_dir = \"/ok\"\n")
	g("toml11", "escape-e", "library_dir = \"\\e\"\n")
	g("toml11", "escape-x", "library_dir = \"\\x41\"\n")
	g("toml11", "key-unicode", "ünï = 1\nlibrary_dir = \"/ok\"\n")
	g("toml11", "inline-comment-in-table", "x = { a = 1, # c\n b = 2 }\n")

	// Two files: precedence, merge and short-circuit order.
	add("merge", "global-only", text("library_dir = \"/g\"\n"), nil)
	add("merge", "project-only", nil, text("library_dir = \"/p\"\n"))
	add("merge", "both-same-field", text("library_dir = \"/g\"\n"), text("library_dir = \"/p\"\n"))
	add("merge", "both-different-fields", text("library_dir = \"/g\"\nrender_dir = \"/gr\"\n"), text("cache_dir = \"/pc\"\nrender_dir = \"/pr\"\n"))
	add("merge", "project-empty-string-overrides", text("library_dir = \"/g\"\n"), text("library_dir = \"\"\n"))
	add("merge", "vcs-global-false-project-true", text("[vcs]\nenabled = false\n"), text("[vcs]\nenabled = true\n"))
	add("merge", "vcs-global-false-project-absent-key", text("[vcs]\nenabled = false\n"), text("library_dir = \"/p\"\n"))
	add("merge", "vcs-global-true-project-false", text("[vcs]\nenabled = true\n"), text("[vcs]\nenabled = false\n"))
	add("merge", "vcs-project-nontable-keeps-global", text("[vcs]\nenabled = false\n"), text("vcs = 1\n"))
	add("merge", "global-bad-type-project-ok", text("library_dir = 1\n"), text("library_dir = \"/p\"\n"))
	add("merge", "global-syntax-error-project-ok", text("library_dir = \n"), text("library_dir = \"/p\"\n"))
	add("merge", "global-ok-project-bad-type", text("library_dir = \"/g\"\n"), text("render_dir = 1\n"))
	add("merge", "global-ok-project-syntax", text("library_dir = \"/g\"\n"), text("render_dir = \n"))
	add("merge", "both-bad-type", text("library_dir = 1\n"), text("render_dir = 1\n"))
	add("merge", "both-syntax", text("a = \n"), text("b = \n"))
	add("merge", "global-type-project-syntax", text("library_dir = 1\n"), text("b = \n"))
	add("merge", "global-syntax-project-type", text("a = \n"), text("library_dir = 1\n"))
	add("merge", "global-empty-project-set", text(""), text("base_dir = \"/pb\"\n"))
	add("merge", "global-bom-project-bom", text("\xef\xbb\xbflibrary_dir = \"/g\"\n"), text("\xef\xbb\xbfbase_dir = \"/p\"\n"))
	add("merge", "targets-both", text("[[targets]]\nname = \"a\"\nskill_root_user = \"/ga\"\n"), text("[[targets]]\nname = \"b\"\nskill_root_user = \"/pb\"\n"))
	add("merge", "targets-global-project-empty", text("[[targets]]\nname = \"a\"\nskill_root_user = \"/ga\"\n"), text("targets = []\n"))
	add("merge", "targets-global-project-other-keys", text("[[targets]]\nname = \"a\"\nskill_root_user = \"/ga\"\n"), text("library_dir = \"/p\"\n"))
	add("merge", "caps-merge", text("[capabilities.codex]\nsubagents = true\nmcp = false\n"), text("[capabilities.codex]\nmcp = true\n[capabilities.claude]\nx = true\n"))

	// Typed targets / capabilities decode.
	t := func(id, body string) { g("targets", id, body) }
	t("one-full", "[[targets]]\nname = \"a\"\ndisplay_name = \"A\"\nbinary_name = \"a-bin\"\nskill_root_user = \"/u\"\nskill_root_project = \"/p\"\nmetadata_file = \"m.yaml\"\nmetadata_template = \"/t\"\noverlay_dir = \"ov\"\n[targets.capabilities]\nsubagents = true\nmcp = false\n")
	t("two", "[[targets]]\nname = \"a\"\nskill_root_user = \"/u\"\n[[targets]]\nname = \"b\"\nskill_root_user = \"/v\"\n")
	t("inline-array", "targets = [{ name = \"a\", skill_root_user = \"/u\" }, { name = \"b\", skill_root_user = \"/v\" }]\n")
	t("inline-caps", "targets = [{ name = \"a\", skill_root_user = \"/u\", capabilities = { mcp = true } }]\n")
	t("empty-array", "targets = []\n")
	t("empty-aot-entry", "[[targets]]\n")
	t("empty-strings", "[[targets]]\nname = \"\"\nskill_root_user = \"\"\n")
	t("unknown-field", "[[targets]]\nname = \"a\"\nfoo = 1\nskill_root_user = \"/u\"\n")
	t("upper-field", "[[targets]]\nName = \"a\"\nskill_root_user = \"/u\"\n")
	t("mixed-case-field", "[[targets]]\nSkill_Root_User = \"/u\"\n")
	t("case-twins", "[[targets]]\nname = \"a\"\nName = \"b\"\n")
	t("upper-top-key", "[[Targets]]\nname = \"a\"\n")
	t("upper-top-key-alt", "[[TARGETS]]\nname = \"a\"\n")
	t("upper-caps-key", "[Capabilities.codex]\nx = true\n")
	t("dash-field", "[[targets]]\nskill-root-user = \"/u\"\n")
	t("targets-table", "[targets]\nname = \"a\"\n")
	t("targets-string", "targets = \"a\"\n")
	t("targets-int", "targets = 1\n")
	t("targets-array-of-ints", "targets = [1, 2]\n")
	t("targets-array-of-arrays", "targets = [[1]]\n")
	t("targets-mixed-array", "targets = [{ name = \"a\" }, 1]\n")
	t("targets-array-of-strings", "targets = [\"a\"]\n")
	t("target-name-int", "[[targets]]\nname = 1\n")
	t("target-name-bool", "[[targets]]\nname = true\n")
	t("target-name-array", "[[targets]]\nname = [\"a\"]\n")
	t("target-name-table", "[[targets]]\n[targets.name]\nx = 1\n")
	t("target-name-datetime", "[[targets]]\nname = 1979-05-27\n")
	t("target-name-float", "[[targets]]\nname = 1.5\n")
	t("target-field-ml", "[[targets]]\nname = \"\"\"a\nb\"\"\"\n")
	t("target-caps-wrong", "[[targets]]\nname = \"a\"\ncapabilities = 1\n")
	t("target-caps-nonbool", "[[targets]]\nname = \"a\"\n[targets.capabilities]\nx = \"yes\"\n")
	t("target-caps-int-bool", "[[targets]]\nname = \"a\"\n[targets.capabilities]\nx = 1\n")
	t("target-caps-nested", "[[targets]]\nname = \"a\"\n[targets.capabilities.sub]\nx = true\n")
	t("target-caps-empty", "[[targets]]\nname = \"a\"\n[targets.capabilities]\n")
	t("target-caps-empty-inline", "[[targets]]\nname = \"a\"\ncapabilities = {}\n")
	t("target-caps-unicode-key", "[[targets]]\nname = \"a\"\n[targets.capabilities]\n\"日本\" = true\n\"a b\" = false\n")
	t("target-caps-key-case", "[[targets]]\nname = \"a\"\n[targets.capabilities]\nMCP = true\nmcp = false\n")
	t("caps-basic", "[capabilities.codex]\nsubagents = true\nmcp = false\n[capabilities.opencode]\nsubagents = false\n")
	t("caps-dotted", "capabilities.codex.mcp = true\n")
	t("caps-inline", "capabilities = { codex = { mcp = true } }\n")
	t("caps-flat-wrong", "[capabilities]\nmcp = true\n")
	t("caps-string-wrong", "capabilities = \"x\"\n")
	t("caps-array-wrong", "capabilities = [1]\n")
	t("caps-array-tables-wrong", "[[capabilities]]\nx = 1\n")
	t("caps-non-bool", "[capabilities.codex]\nmcp = \"true\"\n")
	t("caps-empty-table", "[capabilities]\n")
	t("caps-empty-target", "[capabilities.codex]\n")
	t("caps-dup-target", "[capabilities.codex]\nmcp = true\n[capabilities.codex]\nmcp = false\n")
	t("caps-and-targets", "[capabilities.codex]\nmcp = true\n[[targets]]\nname = \"a\"\nskill_root_user = \"/u\"\n")
	t("fold-kelvin-sign", "[[targets]]\n\"s\u212aill_root_user\" = \"/u\"\n")
	t("fold-long-s", "[[targets]]\n\"\u017fkill_root_user\" = \"/u\"\n")
	t("fold-long-s-twins", "[[targets]]\n\"\u017fkill_root_user\" = \"/a\"\nskill_root_user = \"/b\"\n")
	t("fold-top-targets", "[[\"target\u017f\"]]\nname = \"a\"\n")
	t("fold-top-capabilities", "[\"capabilitie\u017f\".codex]\nmcp = true\n")
	t("fold-ascii-lookalike", "[[targets]]\n\"skill_root_u\u017fer\" = \"/u\"\n\"nam\u0435\" = \"x\"\n")
	t("fold-dotless-i", "[[targets]]\n\"d\u0131splay_name\" = \"D\"\n")
	t("targets-after-other", "library_dir = \"/l\"\n[vcs]\nenabled = false\n[[targets]]\nname = \"a\"\n")
	t("targets-with-bad-config-type", "library_dir = 1\n[[targets]]\nname = \"a\"\n")
	t("targets-unicode", "[[targets]]\nname = \"ünï-日本-😀\"\nskill_root_user = \"/u\"\n")
	t("targets-escape", "[[targets]]\nname = \"a\\tb\\\\c\\\"d\"\nskill_root_user = 'C:\\x'\n")
	t("targets-nested-aot-unknown", "[[targets]]\nname = \"a\"\n[[targets.extra]]\nx = 1\n")
	t("targets-nested-table-unknown", "[[targets]]\nname = \"a\"\n[targets.extra]\nx = 1\n")
	t("targets-aot-then-table", "[[targets]]\nname = \"a\"\n[targets]\nx = 1\n")
	t("targets-bom", "\xef\xbb\xbf[[targets]]\nname = \"a\"\n")
	t("targets-crlf", "[[targets]]\r\nname = \"a\"\r\nskill_root_user = \"/u\"\r\n")
	t("targets-many", func() string {
		var sb strings.Builder
		for i := 0; i < 40; i++ {
			fmt.Fprintf(&sb, "[[targets]]\nname = \"t%d\"\nskill_root_user = \"/u/%d\"\n", i, i)
		}
		return sb.String()
	}())

	// Huge and deep inputs.
	add("huge", "string-20k", &input{Repeat: &repeat{Prefix: "library_dir = \"", Unit: "a", Count: 20000, Suffix: "\"\n"}}, nil)
	add("huge", "string-ml-20k", &input{Repeat: &repeat{Prefix: "library_dir = \"\"\"", Unit: "ab\n", Count: 8000, Suffix: "\"\"\"\n"}}, nil)
	add("huge", "comment-1m", &input{Repeat: &repeat{Prefix: "# ", Unit: "x", Count: 1000000, Suffix: "\nlibrary_dir = \"/ok\"\n"}}, nil)
	add("huge", "dup-keys-repeat", &input{Repeat: &repeat{Prefix: "", Unit: "k = 1\n", Count: 2}}, nil)
	add("huge", "many-unique-keys", func() *input {
		var sb strings.Builder
		for i := 0; i < 5000; i++ {
			fmt.Fprintf(&sb, "k%d = %d\n", i, i)
		}
		sb.WriteString("library_dir = \"/ok\"\n")
		return text(sb.String())
	}(), nil)
	add("huge", "array-depth-100", &input{Repeat: &repeat{Prefix: "x = ", Unit: "[", Count: 100, Suffix: strings.Repeat("]", 100) + "\nlibrary_dir = \"/ok\"\n"}}, nil)
	add("huge", "array-depth-1000", &input{Repeat: &repeat{Prefix: "x = ", Unit: "[", Count: 1000, Suffix: strings.Repeat("]", 1000) + "\nlibrary_dir = \"/ok\"\n"}}, nil)
	add("huge", "array-depth-20000", &input{Repeat: &repeat{Prefix: "x = ", Unit: "[", Count: 20000, Suffix: strings.Repeat("]", 20000) + "\nlibrary_dir = \"/ok\"\n"}}, nil)
	add("huge", "inline-depth-200", &input{Repeat: &repeat{Prefix: "x = ", Unit: "{a=", Count: 200, Suffix: "1" + strings.Repeat("}", 200) + "\nlibrary_dir = \"/ok\"\n"}}, nil)
	add("huge", "inline-depth-5000", &input{Repeat: &repeat{Prefix: "x = ", Unit: "{a=", Count: 5000, Suffix: "1" + strings.Repeat("}", 5000) + "\nlibrary_dir = \"/ok\"\n"}}, nil)
	add("huge", "dotted-key-depth-500", &input{Repeat: &repeat{Prefix: "", Unit: "a.", Count: 500, Suffix: "b = 1\nlibrary_dir = \"/ok\"\n"}}, nil)
	add("huge", "header-depth-2000", &input{Repeat: &repeat{Prefix: "[", Unit: "a.", Count: 2000, Suffix: "b]\nx = 1\nlibrary_dir = \"/nested\"\n"}}, nil)
	add("huge", "long-key", &input{Repeat: &repeat{Prefix: "", Unit: "k", Count: 100000, Suffix: " = 1\nlibrary_dir = \"/ok\"\n"}}, nil)
	add("huge", "int-digits", &input{Repeat: &repeat{Prefix: "x = 1", Unit: "0", Count: 5000, Suffix: "\nlibrary_dir = \"/ok\"\n"}}, nil)
	add("huge", "aot-3000", &input{Repeat: &repeat{Prefix: "", Unit: "[[a]]\nx = 1\n", Count: 3000, Suffix: "library_dir = \"/nested\"\n"}}, nil)
	add("huge", "aot-targets-3000", &input{Repeat: &repeat{Prefix: "", Unit: "[[targets]]\nname = \"t\"\n", Count: 3000}}, nil)

	// Seeded mutation corpus: single and double edits of valid documents.
	out = append(out, mutations()...)
	return out
}

// mutationCount and mutationSeed size the seeded mutation corpus; the committed
// fixture uses the defaults, soak runs override them.
var (
	mutationCount       = 6000
	mutationSeed  int64 = 764
)

func init() {
	seeds = append(seeds,
		`x = 1979-05-27T07:32:00.999-07:00
y = [1979-05-27, 07:32:00, 1979-05-27 07:32:00]
z = { d = 2000-02-29T23:59:59Z }
library_dir = '/l'
[vcs]
enabled = "false"
`,
		`n = [0, -1, +2, 0xff, 0o17, 0b11, 1_000, 1.5, -2.5e-3, inf, -inf, nan, 6.02e23]
s = """\
  a\
  b"""
t = '''
raw \ text'''
library_dir = "\u00e9\U0001F600\t\\"
`,
		`[a]
b = 1
[a.c]
d = 2
[[e]]
f = 1
[[e]]
f = 2
[e.g]
h = 3
[vcs]
enabled = true
`,
		`a = { b = { c = [1, { d = 2 }] }, e = "f" }
"quoted key" = 'v'
'lit.key'.x = 1
library_dir = """x"""
`,
	)
}

var seeds = []string{
	"library_dir = \"/l\"\nrender_dir = '/r'\ncache_dir = \"\"\"/c\"\"\"\nprofiles_dir = \"/p\" # c\nbase_dir = \"/b\"\n\n[vcs]\nenabled = false\n",
	"# header\n[vcs]\nenabled = true\n\n[[targets]]\nname = \"a\"\nskill_root_user = \"/u\"\n[targets.capabilities]\nmcp = true\n",
	"library_dir = \"/l\"\nx = { a = 1, b = [1, 2, { c = 3 }] }\ny.z = 1979-05-27T07:32:00Z\n[t]\nk = 'v'\n",
	"[capabilities.codex]\nsubagents = true\nmcp = false\n[capabilities.opencode]\nsubagents = false\n[[targets]]\nname = \"n\"\n",
	"a = [1, 2.5, \"s\", true]\nb = 0xFF\nc = 1e3\nd = \"\\u00e9\\t\"\nlibrary_dir = \"\"\"\nmulti\nline\"\"\"\n",
	"[a.b]\nc = 1\n[a.d]\ne = 2\n[[f]]\ng = 1\n[[f]]\ng = 2\n[f.h]\ni = 3\n",
	"vcs.enabled = false\nlibrary_dir = \"/l\"\nrender_dir = \"/r\"\ncache_dir = \"/c\"\n",
	"[vcs]\nenabled = \"t\"\n[[targets]]\nname = \"a\"\ndisplay_name = \"A\"\nbinary_name = \"b\"\nskill_root_project = \"/p\"\noverlay_dir = \"o\"\n",
}

var insertAlphabet = []string{
	"\"", "'", "\"\"\"", "'''", "\\", "[", "]", "[[", "]]", "{", "}", ",", ".", "#", "=", " ", "\t", "\n", "\r\n", "\r", "\x00", "\x7f", "\xff", "\u00e9", "\u2028", "\ufeff",
	"0", "1", "_", "-", "+", ":", "e", "E", "T", "Z", "t", "f", "n", "a", "x", "inf", "nan", "true", "false", "0x", "1979-05-27", "07:32:00", "\\u00e9", "\\q", "\\e", "\\x41",
	"library_dir", "vcs", "enabled", "targets", "name", "capabilities",
}

func mutations() []corpusCase {
	rng := rand.New(rand.NewSource(mutationSeed))
	var out []corpusCase
	for n := 0; n < mutationCount; n++ {
		doc := []byte(seeds[rng.Intn(len(seeds))])
		edits := 1 + rng.Intn(3)
		for e := 0; e < edits && len(doc) > 0; e++ {
			switch rng.Intn(5) {
			case 0: // delete range
				i := rng.Intn(len(doc))
				j := i + 1 + rng.Intn(min(6, len(doc)-i))
				doc = append(doc[:i:i], doc[j:]...)
			case 1, 2: // insert token
				i := rng.Intn(len(doc) + 1)
				tok := insertAlphabet[rng.Intn(len(insertAlphabet))]
				doc = append(doc[:i:i], append([]byte(tok), doc[i:]...)...)
			case 3: // replace one byte
				i := rng.Intn(len(doc))
				tok := insertAlphabet[rng.Intn(len(insertAlphabet))]
				doc = append(doc[:i:i], append([]byte(tok), doc[i+1:]...)...)
			default: // duplicate a line
				lines := bytes.SplitAfter(doc, []byte("\n"))
				i := rng.Intn(len(lines))
				dup := append([][]byte{}, lines[:i+1]...)
				dup = append(dup, lines[i:]...)
				doc = bytes.Join(dup, nil)
			}
		}
		out = append(out, corpusCase{id: fmt.Sprintf("mutation/%04d", n), group: "mutation", global: text(string(doc))})
	}
	return out
}
