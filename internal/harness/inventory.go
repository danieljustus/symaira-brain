package harness

import (
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
)

// InventorySchemaVersion identifies the machine-readable harness inventory
// contract. Consumers should reject unknown major schema versions.
//
// Schema 2 (this version) reports each server as an object with transport
// detail (command/args/url and env-var names) instead of a bare name
// string, so downstream consumers can reach servers without re-parsing
// harness configs. See README "Harness inventory schema".
const InventorySchemaVersion = 2

const maxBindingConfigBytes = 8 << 20

// ConfigInventory describes one global or project-local harness config.
type ConfigInventory struct {
	Path    string       `json:"path"`
	Exists  bool         `json:"exists"`
	Parsed  bool         `json:"parsed"`
	Error   string       `json:"error,omitempty"`
	Servers []ServerInfo `json:"servers"`
}

// HarnessInventory describes the known config locations and the MCP servers
// registered in each location. Project is omitted for harnesses without a
// project-local config or when no project directory was requested.
type HarnessInventory struct {
	Name        Name             `json:"name"`
	DisplayName string           `json:"display_name"`
	Global      ConfigInventory  `json:"global"`
	Project     *ConfigInventory `json:"project,omitempty"`
}

// Inventory is the stable result returned by List. ProjectDir is present when
// the caller asked to inspect project-local configuration.
type Inventory struct {
	SchemaVersion int                `json:"schema_version"`
	ProjectDir    string             `json:"project_dir,omitempty"`
	Harnesses     []HarnessInventory `json:"harnesses"`
}

// Binding records a single symbrain MCP entry bound to profileName in a
// harness config file.
type Binding struct {
	Harness Name
	Path    string
	Profile string
}

// BindingScanError records a config that could not be safely inspected.
type BindingScanError struct {
	Harness Name
	Path    string
	Error   string
}

// BindingScan is the complete result of a profile binding scan. Missing
// configuration files are normal; present unsafe, unreadable, or malformed
// files are returned as errors so callers can fail closed.
type BindingScan struct {
	Bindings []Binding
	Errors   []BindingScanError
}

// List inspects every MCP-installable harness without modifying any config file.
// Missing and malformed configs are represented in the result rather than
// returned as an aggregate error, so consumers can inspect the whole machine.
func List(projectDir string) Inventory {
	if projectDir != "" {
		projectDir = filepath.Clean(projectDir)
	}

	result := Inventory{
		SchemaVersion: InventorySchemaVersion,
		ProjectDir:    projectDir,
		Harnesses:     make([]HarnessInventory, 0, len(All)),
	}
	for _, h := range All {
		if !h.SupportsMCPInstall {
			continue
		}
		entry := HarnessInventory{
			Name:        h.Name,
			DisplayName: h.DisplayName,
			Global:      inspectConfig(h, resolveConfigPath(h.ConfigPath)),
		}
		if h.SupportsProject && projectDir != "" {
			entry.Project = ptr(inspectConfig(h, h.ProjectConfigPath(projectDir)))
		}
		result.Harnesses = append(result.Harnesses, entry)
	}
	return result
}

// ProfileBindings scans every registered harness (global and project-local)
// and returns both discovered bindings and configurations that could not be
// safely inspected. Missing configs are omitted; all other scan failures are
// retained so non-forced removal can fail closed.
func ProfileBindings(profileName string, projectDir string) BindingScan {
	var scan BindingScan
	for _, h := range All {
		if !h.SupportsMCPInstall {
			continue
		}
		globalPath, err := h.ConfigPath()
		if err != nil {
			scan.Errors = append(scan.Errors, BindingScanError{
				Harness: h.Name,
				Path:    "<unresolved>",
				Error:   "resolve config path: " + err.Error(),
			})
		} else if b, err := bindingAt(h, globalPath, profileName); err != nil {
			if !errors.Is(err, os.ErrNotExist) {
				scan.Errors = append(scan.Errors, BindingScanError{Harness: h.Name, Path: globalPath, Error: err.Error()})
			}
		} else if b != nil {
			scan.Bindings = append(scan.Bindings, *b)
		}
		if h.SupportsProject && projectDir != "" {
			projectRoot := filepath.Clean(projectDir)
			projectPath := h.ProjectConfigPath(projectRoot)
			if err := rejectSymlinkedProjectRoot(projectRoot); err != nil {
				scan.Errors = append(scan.Errors, BindingScanError{Harness: h.Name, Path: projectPath, Error: err.Error()})
			} else if b, err := bindingAt(h, projectPath, profileName); err != nil {
				if !errors.Is(err, os.ErrNotExist) {
					scan.Errors = append(scan.Errors, BindingScanError{Harness: h.Name, Path: projectPath, Error: err.Error()})
				}
			} else if b != nil {
				scan.Bindings = append(scan.Bindings, *b)
			}
		}
	}
	sort.Slice(scan.Bindings, func(i, j int) bool {
		if scan.Bindings[i].Harness != scan.Bindings[j].Harness {
			return scan.Bindings[i].Harness < scan.Bindings[j].Harness
		}
		return scan.Bindings[i].Path < scan.Bindings[j].Path
	})
	sort.Slice(scan.Errors, func(i, j int) bool {
		left, right := scan.Errors[i], scan.Errors[j]
		if left.Harness != right.Harness {
			return left.Harness < right.Harness
		}
		if left.Path != right.Path {
			return left.Path < right.Path
		}
		return left.Error < right.Error
	})
	return scan
}

func bindingAt(h Harness, path, profileName string) (*Binding, error) {
	data, err := readConfigFile(path)
	if err != nil {
		return nil, err
	}
	doc, err := Parse(h, data)
	if err != nil {
		return nil, fmt.Errorf("parse configuration: %w", err)
	}
	entry, ok := doc.Server(ServerName)
	if !ok || !entry.IsSymbrain() {
		return nil, nil
	}
	profile, ok := entry.Profile()
	if !ok || profile != profileName {
		return nil, nil
	}
	return &Binding{Harness: h.Name, Path: path, Profile: profile}, nil
}

func resolveConfigPath(resolve func() (string, error)) string {
	path, err := resolve()
	if err != nil {
		return ""
	}
	return path
}

func inspectConfig(h Harness, path string) ConfigInventory {
	result := ConfigInventory{
		Path:    path,
		Servers: []ServerInfo{},
	}
	if path == "" {
		result.Error = "resolve config path"
		return result
	}

	data, err := readConfigFile(path)
	if err != nil {
		if !errors.Is(err, os.ErrNotExist) {
			result.Error = err.Error()
		}
		return result
	}
	result.Exists = true

	doc, err := Parse(h, data)
	if err != nil {
		result.Error = err.Error()
		return result
	}
	result.Parsed = true
	for _, name := range doc.ServerNames() {
		if info, ok := doc.ServerInfo(name); ok {
			result.Servers = append(result.Servers, info)
		}
	}
	return result
}

func readConfigFile(path string) ([]byte, error) {
	file, err := openConfigFile(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil {
		return nil, fmt.Errorf("stat configuration: %w", err)
	}
	if !info.Mode().IsRegular() {
		return nil, fmt.Errorf("configuration target is not a regular file")
	}
	if info.Size() > maxBindingConfigBytes {
		return nil, fmt.Errorf("configuration target exceeds maximum size of %d bytes", maxBindingConfigBytes)
	}
	data, err := io.ReadAll(io.LimitReader(file, maxBindingConfigBytes+1))
	if err != nil {
		return nil, fmt.Errorf("read configuration: %w", err)
	}
	if len(data) > maxBindingConfigBytes {
		return nil, fmt.Errorf("configuration target exceeds maximum size of %d bytes", maxBindingConfigBytes)
	}
	return data, nil
}

func rejectSymlinkedProjectRoot(path string) error {
	info, err := os.Lstat(path)
	if err != nil {
		return err
	}
	if info.Mode()&os.ModeSymlink != 0 || !info.IsDir() {
		return fmt.Errorf("project root is not a real directory")
	}
	return nil
}

func ptr[T any](value T) *T {
	return &value
}

func sortedNames(names []string) []string {
	result := append([]string(nil), names...)
	sort.Strings(result)
	return result
}
