package main

import (
	"strings"
	"testing"
)

func TestExecutableName(t *testing.T) {
	tests := []struct {
		name string
		goos string
		want string
	}{
		{name: "symbrain-go", goos: "windows", want: "symbrain-go.exe"},
		{name: "symbrain-go.exe", goos: "windows", want: "symbrain-go.exe"},
		{name: "symbrain-go", goos: "darwin", want: "symbrain-go"},
	}
	for _, test := range tests {
		t.Run(test.goos+"/"+test.name, func(t *testing.T) {
			if got := executableName(test.name, test.goos); got != test.want {
				t.Fatalf("executableName(%q, %q) = %q, want %q", test.name, test.goos, got, test.want)
			}
		})
	}
}

func TestSetEnvReplacesInheritedNameCaseInsensitively(t *testing.T) {
	env := []string{"USERPROFILE=C:\\Users\\runner", "UserProfile=C:\\Users\\other", "PATH=C:\\Windows"}
	got := setEnv(env, "USERPROFILE", `C:\oracle\home`)
	var values []string
	for _, entry := range got {
		if strings.EqualFold(strings.SplitN(entry, "=", 2)[0], "USERPROFILE") {
			values = append(values, entry)
		}
	}
	if len(values) != 1 || values[0] != `USERPROFILE=C:\oracle\home` {
		t.Fatalf("USERPROFILE entries = %#v, want one isolated value", values)
	}
}
