package harness

import (
	"testing"
)

func TestExplicitTransport(t *testing.T) {
	cases := []struct {
		transport, typ, url string
		want                string
	}{
		{"stdio", "", "", "stdio"},
		{"http", "", "", "http"},
		{"", "stdio", "", "stdio"},
		{"", "http", "", "http"},
		{"", "", "http://localhost:9000", "http"},
		{"", "", "", "stdio"},
		{"sse", "", "", "sse"},
		{"", "sse", "http://localhost:9000", "sse"}, // explicit transport wins over url
	}
	for _, tc := range cases {
		t.Run(tc.transport+"_"+tc.typ+"_"+tc.url, func(t *testing.T) {
			got := explicitTransport(tc.transport, tc.typ, tc.url)
			if got != tc.want {
				t.Errorf("explicitTransport(%q, %q, %q) = %q, want %q",
					tc.transport, tc.typ, tc.url, got, tc.want)
			}
		})
	}
}

func TestServerInfoFromMap(t *testing.T) {
	cases := []struct {
		name string
		m    map[string]any
		want ServerInfo
	}{
		{
			name: "full entry",
			m: map[string]any{
				"name":      "web",
				"command":   "uvx",
				"args":      []any{"mcp-server", "--port", "9000"},
				"url":       "http://localhost:9000",
				"transport": "http",
				"env":       map[string]any{"API_KEY": "v0", "TOKEN": "t1"},
			},
			want: ServerInfo{
				Name: "web", Command: "uvx",
				Args: []string{"mcp-server", "--port", "9000"},
				URL:  "http://localhost:9000", Transport: "http",
				EnvNames: []string{"API_KEY", "TOKEN"},
			},
		},
		{
			name: "url-only infers http transport",
			m:    map[string]any{"name": "remote", "url": "http://localhost:8080/mcp"},
			want: ServerInfo{Name: "remote", URL: "http://localhost:8080/mcp", Transport: "http"},
		},
		{
			name: "command-only infers stdio transport",
			m:    map[string]any{"name": "local", "command": "symvault"},
			want: ServerInfo{Name: "local", Command: "symvault", Transport: "stdio"},
		},
		{
			name: "type field used as transport fallback",
			m:    map[string]any{"name": "sse-server", "type": "sse", "url": "http://localhost:9000"},
			want: ServerInfo{Name: "sse-server", URL: "http://localhost:9000", Transport: "sse"},
		},
		{
			name: "args with non-string entries are skipped",
			m:    map[string]any{"name": "bad-args", "command": "x", "args": []any{123, "good"}},
			want: ServerInfo{Name: "bad-args", Command: "x", Args: []string{"good"}, Transport: "stdio"},
		},
		{
			name: "no command or url defaults to stdio",
			m:    map[string]any{"name": "bare"},
			want: ServerInfo{Name: "bare", Transport: "stdio"},
		},
		{
			name: "env values are never exposed only names",
			m: map[string]any{
				"name": "vaultish", "command": "symvault",
				"env": map[string]any{"SECRET": "leaked-key-should-not-appear"},
			},
			want: ServerInfo{Name: "vaultish", Command: "symvault", Transport: "stdio", EnvNames: []string{"SECRET"}},
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := serverInfoFromMap(tc.m, tc.want.Name)
			if got.Name != tc.want.Name {
				t.Errorf("Name = %q, want %q", got.Name, tc.want.Name)
			}
			if got.Command != tc.want.Command {
				t.Errorf("Command = %q, want %q", got.Command, tc.want.Command)
			}
			if got.URL != tc.want.URL {
				t.Errorf("URL = %q, want %q", got.URL, tc.want.URL)
			}
			if got.Transport != tc.want.Transport {
				t.Errorf("Transport = %q, want %q", got.Transport, tc.want.Transport)
			}
			if len(got.Args) != len(tc.want.Args) {
				t.Fatalf("Args = %v, want %v", got.Args, tc.want.Args)
			}
			for i := range got.Args {
				if got.Args[i] != tc.want.Args[i] {
					t.Errorf("Args[%d] = %q, want %q", i, got.Args[i], tc.want.Args[i])
				}
			}
			if len(got.EnvNames) != len(tc.want.EnvNames) {
				t.Fatalf("EnvNames = %v, want %v", got.EnvNames, tc.want.EnvNames)
			}
			for i := range got.EnvNames {
				if got.EnvNames[i] != tc.want.EnvNames[i] {
					t.Errorf("EnvNames[%d] = %q, want %q", i, got.EnvNames[i], tc.want.EnvNames[i])
				}
			}
		})
	}
}

func TestServerInfoFromOrderedMap(t *testing.T) {
	cases := []struct {
		name string
		m    *orderedMap
		want ServerInfo
	}{
		{
			name: "full entry",
			m: func() *orderedMap {
				m := newOrderedMap()
				m.set("name", "web")
				m.set("command", "uvx")
				m.set("args", []any{"mcp-server"})
				m.set("url", "http://localhost:9000")
				m.set("transport", "http")
				m.set("env", map[string]any{"API_KEY": "v0"})
				return m
			}(),
			want: ServerInfo{
				Name: "web", Command: "uvx", Args: []string{"mcp-server"},
				URL: "http://localhost:9000", Transport: "http", EnvNames: []string{"API_KEY"},
			},
		},
		{
			name: "url-only infers http",
			m: func() *orderedMap {
				m := newOrderedMap()
				m.set("url", "http://localhost:8080/mcp")
				return m
			}(),
			want: ServerInfo{URL: "http://localhost:8080/mcp", Transport: "http"},
		},
		{
			name: "command-only infers stdio",
			m: func() *orderedMap {
				m := newOrderedMap()
				m.set("command", "symvault")
				return m
			}(),
			want: ServerInfo{Command: "symvault", Transport: "stdio"},
		},
		{
			name: "type field used as transport fallback",
			m: func() *orderedMap {
				m := newOrderedMap()
				m.set("type", "sse")
				m.set("url", "http://localhost:9000")
				return m
			}(),
			want: ServerInfo{URL: "http://localhost:9000", Transport: "sse"},
		},
		{
			// env as orderedMap extracts names and sorts them alphabetically.
			name: "env as orderedMap extracts sorted names",
			m: func() *orderedMap {
				env := newOrderedMap()
				env.set("TOKEN", "t0")
				env.set("KEY", "k0")
				m := newOrderedMap()
				m.set("command", "x")
				m.set("env", env)
				return m
			}(),
			want: ServerInfo{Command: "x", Transport: "stdio", EnvNames: []string{"KEY", "TOKEN"}},
		},
		{
			name: "env as plain map extracts names",
			m: func() *orderedMap {
				m := newOrderedMap()
				m.set("command", "x")
				m.set("env", map[string]any{"A": "1", "B": "2"})
				return m
			}(),
			want: ServerInfo{Command: "x", Transport: "stdio", EnvNames: []string{"A", "B"}},
		},
		{
			name: "no command or url defaults to stdio",
			m: func() *orderedMap {
				m := newOrderedMap()
				m.set("name", "bare")
				return m
			}(),
			want: ServerInfo{Name: "bare", Transport: "stdio"},
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := serverInfoFromOrderedMap(tc.m, tc.want.Name)
			if got.Name != tc.want.Name {
				t.Errorf("Name = %q, want %q", got.Name, tc.want.Name)
			}
			if got.Command != tc.want.Command {
				t.Errorf("Command = %q, want %q", got.Command, tc.want.Command)
			}
			if got.URL != tc.want.URL {
				t.Errorf("URL = %q, want %q", got.URL, tc.want.URL)
			}
			if got.Transport != tc.want.Transport {
				t.Errorf("Transport = %q, want %q", got.Transport, tc.want.Transport)
			}
			if len(got.Args) != len(tc.want.Args) {
				t.Fatalf("Args = %v, want %v", got.Args, tc.want.Args)
			}
			for i := range got.Args {
				if got.Args[i] != tc.want.Args[i] {
					t.Errorf("Args[%d] = %q, want %q", i, got.Args[i], tc.want.Args[i])
				}
			}
			if len(got.EnvNames) != len(tc.want.EnvNames) {
				t.Fatalf("EnvNames = %v, want %v", got.EnvNames, tc.want.EnvNames)
			}
			for i := range got.EnvNames {
				if got.EnvNames[i] != tc.want.EnvNames[i] {
					t.Errorf("EnvNames[%d] = %q, want %q", i, got.EnvNames[i], tc.want.EnvNames[i])
				}
			}
		})
	}
}

func TestGenericDocument_ServerInfo(t *testing.T) {
	cases := []struct {
		name       string
		root       map[string]any
		serversKey string
		lookup     string
		want       ServerInfo
		wantOK     bool
	}{
		{
			name:       "nil servers returns false",
			root:       map[string]any{},
			serversKey: "mcpServers",
			lookup:     "web",
			want:       ServerInfo{},
			wantOK:     false,
		},
		{
			name: "missing server returns false",
			root: map[string]any{
				"mcpServers": map[string]any{"other": map[string]any{"command": "other"}},
			},
			serversKey: "mcpServers",
			lookup:     "web",
			want:       ServerInfo{},
			wantOK:     false,
		},
		{
			name: "wrong type returns false",
			root: map[string]any{
				"mcpServers": map[string]any{"web": "not-a-map"},
			},
			serversKey: "mcpServers",
			lookup:     "web",
			want:       ServerInfo{},
			wantOK:     false,
		},
		{
			name: "successful read",
			root: map[string]any{
				"mcpServers": map[string]any{
					"web": map[string]any{
						"command":   "uvx",
						"args":      []any{"mcp-server"},
						"url":       "http://localhost:9000",
						"transport": "http",
					},
				},
			},
			serversKey: "mcpServers",
			lookup:     "web",
			want: ServerInfo{
				Name: "web", Command: "uvx",
				Args: []string{"mcp-server"},
				URL:  "http://localhost:9000", Transport: "http",
			},
			wantOK: true,
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			d := &genericDocument{root: tc.root, serversKey: tc.serversKey}
			got, ok := d.ServerInfo(tc.lookup)
			if ok != tc.wantOK {
				t.Fatalf("ok = %v, want %v", ok, tc.wantOK)
			}
			if !ok {
				return
			}
			if got.Name != tc.want.Name {
				t.Errorf("Name = %q, want %q", got.Name, tc.want.Name)
			}
			if got.Command != tc.want.Command {
				t.Errorf("Command = %q, want %q", got.Command, tc.want.Command)
			}
			if got.Transport != tc.want.Transport {
				t.Errorf("Transport = %q, want %q", got.Transport, tc.want.Transport)
			}
		})
	}
}

func TestJSONDocument_ServerInfo(t *testing.T) {
	cases := []struct {
		name       string
		root       *orderedMap
		serversKey string
		lookup     string
		want       ServerInfo
		wantOK     bool
	}{
		{
			name:       "nil servers returns false",
			root:       newOrderedMap(),
			serversKey: "mcpServers",
			lookup:     "web",
			want:       ServerInfo{},
			wantOK:     false,
		},
		{
			name: "missing server returns false",
			root: func() *orderedMap {
				servers := newOrderedMap()
				other := newOrderedMap()
				other.set("command", "other")
				servers.set("other", other)
				root := newOrderedMap()
				root.set("mcpServers", servers)
				return root
			}(),
			serversKey: "mcpServers",
			lookup:     "web",
			want:       ServerInfo{},
			wantOK:     false,
		},
		{
			name: "wrong type returns false",
			root: func() *orderedMap {
				servers := newOrderedMap()
				servers.set("web", "not-a-map")
				root := newOrderedMap()
				root.set("mcpServers", servers)
				return root
			}(),
			serversKey: "mcpServers",
			lookup:     "web",
			want:       ServerInfo{},
			wantOK:     false,
		},
		{
			name: "successful read",
			root: func() *orderedMap {
				web := newOrderedMap()
				web.set("command", "uvx")
				web.set("args", []any{"mcp-server"})
				web.set("url", "http://localhost:9000")
				web.set("transport", "http")
				servers := newOrderedMap()
				servers.set("web", web)
				root := newOrderedMap()
				root.set("mcpServers", servers)
				return root
			}(),
			serversKey: "mcpServers",
			lookup:     "web",
			want: ServerInfo{
				Name: "web", Command: "uvx", Args: []string{"mcp-server"},
				URL: "http://localhost:9000", Transport: "http",
			},
			wantOK: true,
		},
		{
			// Transport is inferred from url when no explicit transport/type.
			name: "url-only infers http transport",
			root: func() *orderedMap {
				web := newOrderedMap()
				web.set("url", "http://localhost:8080/mcp")
				servers := newOrderedMap()
				servers.set("web", web)
				root := newOrderedMap()
				root.set("mcpServers", servers)
				return root
			}(),
			serversKey: "mcpServers",
			lookup:     "web",
			want:       ServerInfo{Name: "web", URL: "http://localhost:8080/mcp", Transport: "http"},
			wantOK:     true,
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			d := newJSONDocument(tc.root, tc.serversKey)
			got, ok := d.ServerInfo(tc.lookup)
			if ok != tc.wantOK {
				t.Fatalf("ok = %v, want %v", ok, tc.wantOK)
			}
			if !ok {
				return
			}
			if got.Name != tc.want.Name {
				t.Errorf("Name = %q, want %q", got.Name, tc.want.Name)
			}
			if got.Command != tc.want.Command {
				t.Errorf("Command = %q, want %q", got.Command, tc.want.Command)
			}
			if got.Transport != tc.want.Transport {
				t.Errorf("Transport = %q, want %q", got.Transport, tc.want.Transport)
			}
			if got.URL != tc.want.URL {
				t.Errorf("URL = %q, want %q", got.URL, tc.want.URL)
			}
		})
	}
}
