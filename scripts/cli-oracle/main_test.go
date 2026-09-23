package main

import "testing"

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
