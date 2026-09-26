package daemon

import (
	"strings"
	"testing"
)

func TestReadURLUsesDaemonNavigationGuard(t *testing.T) {
	runtime := &NavigationRuntime{}
	for _, frame := range []Frame{
		{Cmd: "read", Args: []byte(`{"url":"relative-probe"}`)},
		{Cmd: "read", Args: []byte(`{"url":"data:text/html,unsafe"}`)},
	} {
		err := runtime.guardFrameTarget(frame)
		if err == nil || !strings.Contains(err.Error(), "navigation URL policy: unsupported target") {
			t.Errorf("read target %s: got %v, want URL policy denial", frame.Args, err)
		}
	}
	if err := runtime.guardFrameTarget(Frame{Cmd: "read", Args: []byte(`{}`)}); err != nil {
		t.Fatalf("read current page: %v", err)
	}
}
