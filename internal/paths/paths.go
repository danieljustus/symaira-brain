// Package paths resolves the on-disk locations Symaira Memory and Symaira
// Skills use for configuration, data, and cache files, under one shared XDG
// resolver namespaced by the symbrain app name.
//
// Both cores were absorbed into symbrain from standalone binaries
// (symmemory, symskills) that each rolled their own path convention: memory
// respected XDG_CONFIG_HOME/XDG_DATA_HOME under its own app name
// ("symmemory"), while skills hardcoded $HOME/.config/symskills and
// ~/.local/share/symskills and ignored XDG_* entirely. That left one binary
// scattering state over three app names with inconsistent env handling.
//
// This package replaces both with a single convention:
// $XDG_*_HOME/symbrain/<component>, with a read-only fallback to each
// core's legacy standalone directory when the new location does not exist
// yet but the legacy one does — so an existing install keeps working
// without a migration step. No data is moved; callers just keep reading
// from wherever it already lives.
package paths

import (
	"os"
	"path/filepath"
)

// AppName is the shared XDG namespace both cores resolve under.
const AppName = "symbrain"

// Component names segmenting the shared symbrain namespace.
const (
	ComponentMemory = "memory"
	ComponentSkills = "skills"
)

// Legacy standalone app names each core used before being absorbed into
// symbrain. Kept only for read-only fallback resolution; never write here.
const (
	legacyMemoryApp = "symmemory"
	legacySkillsApp = "symskills"
)

// Location is a resolved directory plus whether it was found under a
// core's legacy standalone location instead of the current
// symbrain-namespaced one. Callers that only need the directory can use
// .Dir directly. Legacy is intended for a future `symbrain doctor` (#425)
// to surface as a migrate-to-current-layout hint; that reporting is out of
// scope here.
type Location struct {
	Dir    string
	Legacy bool
}

// MemoryConfigDir resolves Symaira Memory's configuration directory:
// $XDG_CONFIG_HOME/symbrain/memory, or ~/.config/symbrain/memory when
// XDG_CONFIG_HOME is unset. Falls back to the legacy
// $XDG_CONFIG_HOME/symmemory (or ~/.config/symmemory) when only that
// exists.
func MemoryConfigDir() (Location, error) {
	return resolve("XDG_CONFIG_HOME", ".config", ComponentMemory, legacyMemoryApp)
}

// MemoryDataDir resolves Symaira Memory's data directory:
// $XDG_DATA_HOME/symbrain/memory, or ~/.local/share/symbrain/memory when
// XDG_DATA_HOME is unset. Falls back to the legacy
// $XDG_DATA_HOME/symmemory (or ~/.local/share/symmemory) when only that
// exists.
func MemoryDataDir() (Location, error) {
	return resolve("XDG_DATA_HOME", filepath.Join(".local", "share"), ComponentMemory, legacyMemoryApp)
}

// SkillsConfigDir resolves Symaira Skills' configuration directory:
// $XDG_CONFIG_HOME/symbrain/skills, or ~/.config/symbrain/skills when
// XDG_CONFIG_HOME is unset. Falls back to the legacy
// $XDG_CONFIG_HOME/symskills (or ~/.config/symskills) when only that
// exists. Legacy symskills previously ignored XDG_CONFIG_HOME entirely and
// always resolved to $HOME/.config/symskills; that env var is now honored
// for both the current and legacy locations.
func SkillsConfigDir() (Location, error) {
	return resolve("XDG_CONFIG_HOME", ".config", ComponentSkills, legacySkillsApp)
}

// SkillsDataDir resolves Symaira Skills' data directory:
// $XDG_DATA_HOME/symbrain/skills, or ~/.local/share/symbrain/skills when
// XDG_DATA_HOME is unset. Falls back to the legacy $XDG_DATA_HOME/symskills
// (or ~/.local/share/symskills) when only that exists. Legacy symskills
// previously ignored XDG_DATA_HOME entirely.
func SkillsDataDir() (Location, error) {
	return resolve("XDG_DATA_HOME", filepath.Join(".local", "share"), ComponentSkills, legacySkillsApp)
}

// SkillsCacheDir resolves Symaira Skills' cache directory:
// $XDG_CACHE_HOME/symbrain/skills, or ~/.cache/symbrain/skills when
// XDG_CACHE_HOME is unset. Falls back to the legacy $XDG_CACHE_HOME/symskills
// (or ~/.cache/symskills) when only that exists. Legacy symskills previously
// ignored XDG_CACHE_HOME entirely.
func SkillsCacheDir() (Location, error) {
	return resolve("XDG_CACHE_HOME", ".cache", ComponentSkills, legacySkillsApp)
}

// resolve computes the current symbrain-namespaced directory
// (base/symbrain/component) and, if that does not exist on disk but the
// legacy standalone directory (base/legacyApp) does, returns the legacy
// directory instead with Legacy set. base is $envVar when it is set to an
// absolute path, else $HOME/fallbackRel — matching the XDG Base Directory
// Specification, which says a relative value for one of these variables
// must be ignored.
func resolve(envVar, fallbackRel, component, legacyApp string) (Location, error) {
	base, err := baseDir(envVar, fallbackRel)
	if err != nil {
		return Location{}, err
	}

	current := filepath.Join(base, AppName, component)
	if dirExists(current) {
		return Location{Dir: current}, nil
	}

	legacy := filepath.Join(base, legacyApp)
	if dirExists(legacy) {
		return Location{Dir: legacy, Legacy: true}, nil
	}

	return Location{Dir: current}, nil
}

func baseDir(envVar, fallbackRel string) (string, error) {
	if v := os.Getenv(envVar); filepath.IsAbs(v) {
		return v, nil
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, fallbackRel), nil
}

func dirExists(path string) bool {
	info, err := os.Stat(path)
	return err == nil && info.IsDir()
}
