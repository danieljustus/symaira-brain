// Package archguard enforces allowed import directions between symguard
// internal packages. The intended dependency planes are:
//
//	model → policy → approval → audit
//	model → sequence → policy
//	proposal → model, config, audit (approval layer: persisted policy changes)
//	model → output
//	config → (standalone, consumed by all)
//	grant → (standalone leaf, consumed by approval and policy)
//	discovery → (standalone, consumed by scan command)
//	capability → (standalone leaf, consumed by CLI and future MCP proxy;
//	its scope ceiling helper lives in policy, which must not import it)
//
// No package in a higher plane may import a package from a lower plane
// (e.g. audit must not import policy). Utility packages (config, grant,
// discovery, update) are leaf nodes — nothing in the dependency chain
// imports them except the approval/policy layers and the CLI entrypoint.
package archguard

import (
	"go/parser"
	"go/token"
	"io/fs"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

// Module import prefixes after the guard module was dissolved into the
// root module (ADR 0001, D6). The process boundary that used to enforce
// the Brain/Guard separation is gone, so the rule is carried here.
const (
	brainInternalPrefix = "github.com/danieljustus/symaira-brain/internal/"
	guardInternalPrefix = "github.com/danieljustus/symaira-brain/guard/internal/"
)

// AllowedImports defines which packages may import which other packages.
// Key: importing package (relative to internal/).
// Value: set of allowed imported packages (relative to internal/).
// An empty allowed set means the package must not import anything from internal/.
//
// The root module prefix is stripped before comparison.
type AllowedImports map[string]map[string]bool

// DefaultAllowed defines the canonical dependency graph.
var DefaultAllowed = AllowedImports{
	"model":      {},
	"policy":     {"model": true, "grant": true, "sequence": true},
	"sequence":   {"model": true},
	"approval":   {"model": true, "grant": true},
	"proposal":   {"model": true, "config": true, "audit": true},
	"audit":      {"model": true},
	"output":     {},
	"config":     {},
	"grant":      {"config": true},
	"discovery":  {"config": true},
	"update":     {},
	"capability": {"config": true},
}

// Check returns a list of violations where an internal package imports
// another internal package not in its allowed set.
func (a AllowedImports) Check(root string) []string {
	var violations []string

	err := filepath.WalkDir(root, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() || !strings.HasSuffix(path, ".go") ||
			strings.HasSuffix(path, "_test.go") {
			return nil
		}

		fset := token.NewFileSet()
		f, err := parser.ParseFile(fset, path, nil, parser.ImportsOnly)
		if err != nil {
			return nil // skip unparseable files
		}

		pkg := filepath.Dir(path)
		pkgRel := strings.TrimPrefix(pkg, root+"/")

		allowed := a[pkgRel]
		if allowed == nil {
			// Unknown package — allow everything (no constraint yet).
			return nil
		}

		for _, imp := range f.Imports {
			impPath := strings.Trim(imp.Path.Value, `"`)
			// Check if it imports another internal/ package
			if !strings.HasPrefix(impPath, guardInternalPrefix) {
				continue
			}
			impRel := strings.TrimPrefix(impPath, guardInternalPrefix)
			if !allowed[impRel] {
				violations = append(violations,
					path+": imports "+impRel+" (not in allowed set for "+pkgRel+")")
			}
		}
		return nil
	})
	if err != nil {
		return []string{"walk error: " + err.Error()}
	}

	sort.Strings(violations)
	return violations
}

// CheckBoundary returns violations of the Brain/Guard separation, which the
// single-module merge (ADR 0001, D6) no longer enforces by a process
// boundary: brain internal packages must not import anything under guard/,
// and guard internal packages must not import brain's internal packages.
// The CLI entrypoint (cmd/symbrain) wiring into guard/cmd is deliberately
// outside both internal trees and therefore not constrained.
func CheckBoundary(repoRoot string) []string {
	var violations []string

	for _, dir := range []string{"internal", filepath.Join("guard", "internal")} {
		root := filepath.Join(repoRoot, dir)
		if _, err := os.Stat(root); err != nil {
			continue
		}
		_ = filepath.WalkDir(root, func(path string, d fs.DirEntry, err error) error {
			if err != nil {
				return err
			}
			if d.IsDir() || !strings.HasSuffix(path, ".go") ||
				strings.HasSuffix(path, "_test.go") {
				return nil
			}
			fset := token.NewFileSet()
			f, perr := parser.ParseFile(fset, path, nil, parser.ImportsOnly)
			if perr != nil {
				return nil // skip unparseable files
			}
			for _, imp := range f.Imports {
				impPath := strings.Trim(imp.Path.Value, `"`)
				if dir == "internal" && strings.HasPrefix(impPath, "github.com/danieljustus/symaira-brain/guard/") {
					violations = append(violations, path+": brain internal imports guard ("+impPath+")")
				}
				if dir == filepath.Join("guard", "internal") && strings.HasPrefix(impPath, brainInternalPrefix) {
					violations = append(violations, path+": guard internal imports brain internal ("+impPath+")")
				}
			}
			return nil
		})
	}

	sort.Strings(violations)
	return violations
}
