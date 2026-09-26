package daemon

import (
	"os"
	"path/filepath"
	"runtime"
	"testing"
)

func TestSocketBaseDirUsesPlatformXDGDefaults(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("USERPROFILE", home)
	t.Setenv("LOCALAPPDATA", filepath.Join(home, "local"))
	t.Setenv("XDG_RUNTIME_DIR", "")
	t.Setenv("XDG_CACHE_HOME", "")

	var want string
	switch runtime.GOOS {
	case "darwin":
		want = filepath.Join(home, "Library", "Caches", "symbrowse", "run")
	case "windows":
		cache, err := os.UserCacheDir()
		if err != nil {
			t.Fatal(err)
		}
		want = filepath.Join(cache, "symbrowse", "run")
	default:
		want = filepath.Join(home, ".cache", "symbrowse", "run")
	}
	got, err := socketBaseDir()
	if err != nil {
		t.Fatal(err)
	}
	if got != want {
		t.Fatalf("socketBaseDir() = %q, want %q", got, want)
	}

	t.Setenv("XDG_RUNTIME_DIR", filepath.Join(home, "runtime"))
	got, err = socketBaseDir()
	if runtime.GOOS == "darwin" {
		if err != nil || got != want {
			t.Fatalf("macOS socket base with XDG_RUNTIME_DIR = %q, %v; want %q", got, err, want)
		}
		return
	}
	if err != nil {
		t.Fatal(err)
	}
	wantRuntime := filepath.Join(home, "runtime", "symbrowse")
	if got != wantRuntime {
		t.Fatalf("socketBaseDir() with XDG_RUNTIME_DIR = %q, want %q", got, wantRuntime)
	}
	t.Setenv("XDG_RUNTIME_DIR", filepath.Join("relative", "runtime"))
	got, err = socketBaseDir()
	if err != nil {
		t.Fatal(err)
	}
	wantRelativeRuntime := filepath.Join("relative", "runtime", "symbrowse")
	if got != wantRelativeRuntime {
		t.Fatalf("socketBaseDir() with relative XDG_RUNTIME_DIR = %q, want %q", got, wantRelativeRuntime)
	}

	if runtime.GOOS == "linux" {
		t.Setenv("XDG_RUNTIME_DIR", "")
		t.Setenv("XDG_CACHE_HOME", filepath.Join("relative", "cache"))
		if _, err := socketBaseDir(); err == nil {
			t.Fatal("relative XDG_CACHE_HOME must be rejected by os.UserCacheDir")
		}
	}
}
