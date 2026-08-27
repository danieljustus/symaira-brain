package harness

import "testing"

func TestNewEntry(t *testing.T) {
	e := NewEntry("personal")
	if e.Command != "symbrain" {
		t.Errorf("Command = %q, want %q", e.Command, "symbrain")
	}
	want := []string{"mcp", "--profile", "personal"}
	if len(e.Args) != len(want) {
		t.Fatalf("Args = %v, want %v", e.Args, want)
	}
	for i := range want {
		if e.Args[i] != want[i] {
			t.Errorf("Args[%d] = %q, want %q", i, e.Args[i], want[i])
		}
	}
}

func TestEntry_IsSymbrain(t *testing.T) {
	cases := []struct {
		name    string
		command string
		want    bool
	}{
		{"bare name", "symbrain", true},
		{"absolute path", "/usr/local/bin/symbrain", true},
		{"relative path", "./bin/symbrain", true},
		{"other tool", "symguard", false},
		{"other tool absolute path", "/usr/local/bin/symguard", false},
		{"empty", "", false},
		{"prefix but not exact basename", "symbrain-legacy", false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			e := Entry{Command: tc.command}
			if got := e.IsSymbrain(); got != tc.want {
				t.Errorf("IsSymbrain(%q) = %v, want %v", tc.command, got, tc.want)
			}
		})
	}
}

func TestEntry_Profile(t *testing.T) {
	cases := []struct {
		name   string
		args   []string
		want   string
		wantOK bool
	}{
		{"space form", []string{"mcp", "--profile", "personal"}, "personal", true},
		{"equals form", []string{"mcp", "--profile=restricted"}, "restricted", true},
		{"missing", []string{"mcp"}, "", false},
		{"dangling flag", []string{"mcp", "--profile"}, "", false},
		{"empty args", nil, "", false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			e := Entry{Command: "symbrain", Args: tc.args}
			got, ok := e.Profile()
			if ok != tc.wantOK {
				t.Fatalf("ok = %v, want %v", ok, tc.wantOK)
			}
			if got != tc.want {
				t.Errorf("Profile() = %q, want %q", got, tc.want)
			}
		})
	}
}

func TestEntry_SupersededCore(t *testing.T) {
	cases := []struct {
		name    string
		command string
		want    string
		wantOK  bool
	}{
		{"bare symmemory", "symmemory", "symmemory", true},
		{"absolute symmemory", "/opt/homebrew/bin/symmemory", "symmemory", true},
		{"managed path symmemory", "/Users/daniel/.symaira/bin/symmemory", "symmemory", true},
		{"bare symskills", "symskills", "symskills", true},
		{"absolute symskills", "/opt/homebrew/bin/symskills", "symskills", true},
		{"symvault untouched", "symvault", "", false},
		{"symbrain untouched", "symbrain", "", false},
		{"renamed entry untouched", "my-memory-tool", "", false},
		{"empty command", "", "", false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			e := Entry{Command: tc.command}
			got, ok := e.SupersededCore()
			if ok != tc.wantOK || got != tc.want {
				t.Errorf("SupersededCore() = %q/%v, want %q/%v", got, ok, tc.want, tc.wantOK)
			}
		})
	}
}
