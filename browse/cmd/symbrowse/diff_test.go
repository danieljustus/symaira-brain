package main

import (
	"bytes"
	"encoding/json"
	"image"
	"image/png"
	"os"
	"path/filepath"
	"testing"

	"github.com/danieljustus/symaira-browse/internal/daemon"
)

func TestDiffCommandsResolveAndUseSessionSocket(t *testing.T) {
	redirectSocketDir(t)
	t.Setenv("SYMBROWSE_NO_AUTOSTART", "1")
	session := uniqueSession()
	path, err := daemon.SocketPath(session)
	if err != nil {
		t.Fatal(err)
	}
	frames := make(chan string, 2)
	captured := filepath.Join(t.TempDir(), "captured.png")
	var pngBytes bytes.Buffer
	if err := png.Encode(&pngBytes, image.NewRGBA(image.Rect(0, 0, 1, 1))); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(captured, pngBytes.Bytes(), 0o600); err != nil {
		t.Fatal(err)
	}
	fakeDaemon(t, path, func(raw []byte) []byte {
		var frame daemon.Frame
		if err := json.Unmarshal(raw, &frame); err != nil {
			return nil
		}
		frames <- frame.Cmd
		var data any
		switch frame.Cmd {
		case "snapshot":
			data = map[string]any{"tree": "current", "refs": map[string]any{}}
		case "screenshot":
			data = map[string]any{"path": captured}
		case "read":
			data = map[string]any{"title": "Fixture", "html": "<p>fixture</p>"}
		default:
			return []byte(`{"success":false,"error":{"code":"unknown_command","message":"unexpected command"}}`)
		}
		response, _ := json.Marshal(map[string]any{"success": true, "data": data, "warnings": []any{}})
		return response
	})

	baseline := filepath.Join(t.TempDir(), "baseline.json")
	if err := os.WriteFile(baseline, []byte(`{"tree":"before","refs":{}}`), 0o600); err != nil {
		t.Fatal(err)
	}
	imageBaseline := filepath.Join(t.TempDir(), "baseline.png")
	if err := os.WriteFile(imageBaseline, pngBytes.Bytes(), 0o600); err != nil {
		t.Fatal(err)
	}
	cases := []struct {
		name string
		args []string
		want []string
	}{
		{"snapshot", []string{"diff", "snapshot", "--baseline", baseline}, []string{"snapshot"}},
		{"screenshot", []string{"diff", "screenshot", "--baseline", imageBaseline}, []string{"screenshot"}},
		{"url", []string{"diff", "url", "https://one.invalid", "https://two.invalid"}, []string{"read", "read"}},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			command := newRootCommand()
			var stdout, stderr bytes.Buffer
			command.SetOut(&stdout)
			command.SetErr(&stderr)
			command.SetArgs(append(test.args, "--session", session, "--json"))
			if err := command.Execute(); err != nil {
				t.Fatalf("Execute() = %v; stderr=%q stdout=%q", err, stderr.String(), stdout.String())
			}
			for _, want := range test.want {
				select {
				case got := <-frames:
					if got != want {
						t.Fatalf("daemon command = %q, want %q", got, want)
					}
				default:
					t.Fatalf("daemon command %q was not sent", want)
				}
			}
			if stdout.Len() == 0 {
				t.Fatal("command returned success without output")
			}
		})
	}
}
