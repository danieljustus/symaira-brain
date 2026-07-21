package harness

// genericDocument implements the map-manipulation half of Document
// (Server/SetServer/RemoveServer) once, shared by both the JSON and TOML
// backends: both decode their config into a plain map[string]any tree, and
// only differ in how that tree is parsed and re-serialized.
type genericDocument struct {
	root       map[string]any
	serversKey string
}

func (d *genericDocument) servers() map[string]any {
	v, ok := d.root[d.serversKey]
	if !ok {
		return nil
	}
	m, _ := v.(map[string]any)
	return m
}

func (d *genericDocument) Server(name string) (Entry, bool) {
	servers := d.servers()
	if servers == nil {
		return Entry{}, false
	}
	raw, ok := servers[name]
	if !ok {
		return Entry{}, false
	}
	m, ok := raw.(map[string]any)
	if !ok {
		return Entry{}, false
	}
	return entryFromMap(m), true
}

func (d *genericDocument) SetServer(name string, entry Entry) {
	servers := d.servers()
	if servers == nil {
		servers = map[string]any{}
		d.root[d.serversKey] = servers
	}
	servers[name] = entryToMap(entry)
}

func (d *genericDocument) RemoveServer(name string) bool {
	servers := d.servers()
	if servers == nil {
		return false
	}
	if _, ok := servers[name]; !ok {
		return false
	}
	delete(servers, name)
	if len(servers) == 0 {
		// Drop the now-empty container key too, so removing the only
		// server entry restores a config that never had the key to begin
		// with — required for install+uninstall to round-trip
		// byte-for-byte back to the original file.
		delete(d.root, d.serversKey)
	}
	return true
}
