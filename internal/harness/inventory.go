package harness

import (
	"errors"
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

// List inspects every registered harness without modifying any config file.
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

	data, err := os.ReadFile(path)
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

func ptr[T any](value T) *T {
	return &value
}

func sortedNames(names []string) []string {
	result := append([]string(nil), names...)
	sort.Strings(result)
	return result
}
