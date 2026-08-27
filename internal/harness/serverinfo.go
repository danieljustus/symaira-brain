package harness

// ServerInfo describes one MCP server entry as registered in a harness
// config: how to reach it (command/args or url) and which environment
// variables it reads. Environment variable *values* are never exposed —
// only their names, so a config carrying a plaintext key cannot leak it
// through the inventory.
type ServerInfo struct {
	Name      string   `json:"name"`
	Transport string   `json:"transport,omitempty"`
	Command   string   `json:"command,omitempty"`
	Args      []string `json:"args,omitempty"`
	URL       string   `json:"url,omitempty"`
	EnvNames  []string `json:"env_names,omitempty"`
}

// serverInfoFromMap builds a ServerInfo from a generic server-table map
// representation (TOML backend). Fields of an unexpected type are left
// zero rather than erroring — a foreign entry simply reports sparse
// detail.
func serverInfoFromMap(m map[string]any, name string) ServerInfo {
	info := ServerInfo{Name: name}
	if s, ok := m["command"].(string); ok {
		info.Command = s
	}
	if raw, ok := m["args"].([]any); ok {
		info.Args = stringArgs(raw)
	}
	if s, ok := m["url"].(string); ok {
		info.URL = s
	}
	info.Transport = explicitTransport(asString(m["transport"]), asString(m["type"]), info.URL)
	if env, ok := m["env"].(map[string]any); ok {
		info.EnvNames = sortedNames(envKeys(env))
	}
	return info
}

// serverInfoFromOrderedMap builds a ServerInfo from an *orderedMap
// (JSON backend).
func serverInfoFromOrderedMap(m *orderedMap, name string) ServerInfo {
	info := ServerInfo{Name: name}
	if s, ok := m.get("command"); ok {
		info.Command = asString(s)
	}
	if raw, ok := m.get("args"); ok {
		if arr, ok := raw.([]any); ok {
			info.Args = stringArgs(arr)
		}
	}
	if s, ok := m.get("url"); ok {
		info.URL = asString(s)
	}
	var transport, typ string
	if s, ok := m.get("transport"); ok {
		transport = asString(s)
	}
	if s, ok := m.get("type"); ok {
		typ = asString(s)
	}
	info.Transport = explicitTransport(transport, typ, info.URL)
	if raw, ok := m.get("env"); ok {
		switch env := raw.(type) {
		case *orderedMap:
			info.EnvNames = sortedNames(append([]string(nil), env.keys...))
		case map[string]any:
			info.EnvNames = sortedNames(envKeys(env))
		}
	}
	return info
}

func asString(v any) string {
	s, _ := v.(string)
	return s
}

func stringArgs(arr []any) []string {
	out := make([]string, 0, len(arr))
	for _, a := range arr {
		if s, ok := a.(string); ok {
			out = append(out, s)
		}
	}
	return out
}

func envKeys(env map[string]any) []string {
	keys := make([]string, 0, len(env))
	for k := range env {
		keys = append(keys, k)
	}
	return keys
}

// explicitTransport reports the server's transport: an explicit
// transport/type field when present, otherwise inferred from the entry
// shape (url-based => http, command-based => stdio).
func explicitTransport(transport, typ, url string) string {
	if transport != "" {
		return transport
	}
	if typ != "" {
		return typ
	}
	if url != "" {
		return "http"
	}
	return "stdio"
}
