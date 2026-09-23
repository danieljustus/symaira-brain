package main

import (
	"bytes"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"time"

	"github.com/danieljustus/symaira-brain/internal/xdg"
)

type TestCase struct {
	ID     string            `json:"id"`
	Env    map[string]string `json:"env"`
	Expect map[string]string `json:"expect"`
}

type Suite struct {
	Cases []TestCase `json:"cases"`
}

func main() {
	check := flag.Bool("check", false, "fail if generated output does not match existing file")
	output := flag.String("output", "rust/symbrain-core/tests/fixtures/oracle_expectations.json", "output path")
	flag.Parse()

	suite := Suite{Cases: buildCases()}
	data, err := json.MarshalIndent(suite, "", "  ")
	if err != nil {
		fmt.Fprintf(os.Stderr, "json marshal: %v\n", err)
		os.Exit(1)
	}
	data = append(data, '\n')

	if *check {
		existing, err := os.ReadFile(*output)
		if err != nil {
			fmt.Fprintf(os.Stderr, "read %s: %v\n", *output, err)
			os.Exit(1)
		}
		if !bytes.Equal(existing, data) {
			fmt.Fprintf(os.Stderr, "%s is out of date; run go run ./scripts/xdg-oracle -check\n", *output)
			os.Exit(1)
		}
		fmt.Printf("PASS: xdg oracle deterministic check passed (0 drift on %d cases)\n", len(suite.Cases))
		return
	}

	if err := os.MkdirAll(filepath.Dir(*output), 0755); err != nil {
		fmt.Fprintf(os.Stderr, "mkdir %s: %v\n", filepath.Dir(*output), err)
		os.Exit(1)
	}

	if err := os.WriteFile(*output, data, 0644); err != nil {
		fmt.Fprintf(os.Stderr, "write %s: %v\n", *output, err)
		os.Exit(1)
	}

	fmt.Printf("Wrote %s (%d cases)\n", *output, len(suite.Cases))
}

func buildCases() []TestCase {
	cases := []TestCase{}

	// Use platform-native absolute paths so the `abs` cases exercise the
	// current filepath implementation instead of Unix slash paths on Windows.
	homeAbs := absoluteTestPath("home", "user")
	configAbs := absoluteTestPath("abs", "config")
	dataAbs := absoluteTestPath("abs", "data")
	cacheAbs := absoluteTestPath("abs", "cache")

	// HOME combinations
	homes := []struct {
		name string
		val  string
	}{{"unset", ""}, {"abs", homeAbs}, {"rel", "relative/home"}, {"empty", ""}}

	// XDG_CONFIG_HOME
	xdgConfigs := []struct {
		name string
		val  string
	}{{"unset", ""}, {"abs", configAbs}, {"rel", "relative/config"}, {"empty", ""}}

	// XDG_DATA_HOME
	xdgData := []struct {
		name string
		val  string
	}{{"unset", ""}, {"abs", dataAbs}, {"rel", "relative/data"}, {"empty", ""}}

	// XDG_CACHE_HOME
	xdgcaches := []struct {
		name string
		val  string
	}{{"unset", ""}, {"abs", cacheAbs}, {"rel", "relative/cache"}, {"empty", ""}}

	for _, home := range homes {
		for _, xdgConfig := range xdgConfigs {
			for _, dataHome := range xdgData {
				for _, cacheHome := range xdgcaches {
					env := map[string]string{}
					if home.val == "" {
						env["HOME"] = ""
					} else {
						env["HOME"] = home.val
					}
					if xdgConfig.val == "" {
						env["XDG_CONFIG_HOME"] = ""
					} else {
						env["XDG_CONFIG_HOME"] = xdgConfig.val
					}
					if dataHome.val == "" {
						env["XDG_DATA_HOME"] = ""
					} else {
						env["XDG_DATA_HOME"] = dataHome.val
					}
					if cacheHome.val == "" {
						env["XDG_CACHE_HOME"] = ""
					} else {
						env["XDG_CACHE_HOME"] = cacheHome.val
					}

					expect := evaluateGo(env)
					cases = append(cases, TestCase{
						ID:     fmt.Sprintf("home=%s,xdg-config=%s,xdg-data=%s,xdg-cache=%s", home.name, xdgConfig.name, dataHome.name, cacheHome.name),
						Env:    env,
						Expect: expect,
					})
				}
			}
		}
	}
	return cases
}

func absoluteTestPath(parts ...string) string {
	root := string(filepath.Separator)
	if runtime.GOOS == "windows" {
		root = `C:\`
	}
	return filepath.Join(append([]string{root}, parts...)...)
}

func evaluateGo(env map[string]string) map[string]string {
	origHome := os.Getenv("HOME")
	origXdgConfig := os.Getenv("XDG_CONFIG_HOME")
	origXdgData := os.Getenv("XDG_DATA_HOME")
	origXdgCache := os.Getenv("XDG_CACHE_HOME")
	origUserProfile, hadUserProfile := os.LookupEnv("USERPROFILE")

	// Set environment
	if v, ok := env["HOME"]; ok && v != "" {
		os.Setenv("HOME", v)
	} else {
		os.Unsetenv("HOME")
	}
	if v, ok := env["XDG_CONFIG_HOME"]; ok && v != "" {
		os.Setenv("XDG_CONFIG_HOME", v)
	} else {
		os.Unsetenv("XDG_CONFIG_HOME")
	}
	if v, ok := env["XDG_DATA_HOME"]; ok && v != "" {
		os.Setenv("XDG_DATA_HOME", v)
	} else {
		os.Unsetenv("XDG_DATA_HOME")
	}
	if v, ok := env["XDG_CACHE_HOME"]; ok && v != "" {
		os.Setenv("XDG_CACHE_HOME", v)
	} else {
		os.Unsetenv("XDG_CACHE_HOME")
	}
	if runtime.GOOS == "windows" {
		if env["HOME"] != "" {
			os.Setenv("USERPROFILE", env["HOME"])
		} else {
			os.Unsetenv("USERPROFILE")
		}
	}

	// Small delay for env to take effect
	time.Sleep(10 * time.Millisecond)

	result := make(map[string]string)

	// All functions that should succeed
	result["config_path"] = xdg.ConfigPath()
	result["config_dir"] = xdg.ConfigDir()
	result["profiles_dir"] = xdg.ProfilesDir()

	// Functions that may return errors
	if d, err := xdg.DataDir(); err != nil {
		result["data_dir"] = "(error)"
	} else {
		result["data_dir"] = d
	}
	if d, err := xdg.AuditDir(); err != nil {
		result["audit_dir"] = "(error)"
	} else {
		result["audit_dir"] = d
	}
	if d, err := xdg.PatternsDir(); err != nil {
		result["patterns_dir"] = "(error)"
	} else {
		result["patterns_dir"] = d
	}
	if c, err := xdg.CacheDir(); err != nil {
		result["cache_dir"] = "(error)"
	} else {
		result["cache_dir"] = c
	}
	if m, err := xdg.ManagedBinDir(); err != nil {
		result["managed_bin_dir"] = "(error)"
	} else {
		result["managed_bin_dir"] = m
	}

	// Restore original env
	os.Setenv("HOME", origHome)
	os.Setenv("XDG_CONFIG_HOME", origXdgConfig)
	os.Setenv("XDG_DATA_HOME", origXdgData)
	os.Setenv("XDG_CACHE_HOME", origXdgCache)
	if runtime.GOOS == "windows" {
		if hadUserProfile {
			os.Setenv("USERPROFILE", origUserProfile)
		} else {
			os.Unsetenv("USERPROFILE")
		}
	}

	return result
}
