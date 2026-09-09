package main

import (
	"bytes"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/policy"
	"github.com/danieljustus/symaira-brain/internal/profile"
)

func runProfileCase(tc ProfileTestCase) ProfileExpectation {
	home, err := os.MkdirTemp("", "oracle-home")
	if err != nil {
		return ProfileExpectation{ID: tc.ID, Success: false, ErrorSubstr: err.Error()}
	}
	defer os.RemoveAll(home)

	// Keep the oracle hermetic: configkit honors XDG_CONFIG_HOME when it is
	// set (as on some Linux runners), so HOME alone is not enough to isolate
	// profile.Load from the runner's configuration.
	origHome := os.Getenv("HOME")
	origXDGConfigHome := os.Getenv("XDG_CONFIG_HOME")
	os.Setenv("HOME", home)
	os.Setenv("XDG_CONFIG_HOME", filepath.Join(home, ".config"))
	defer os.Setenv("HOME", origHome)
	defer os.Setenv("XDG_CONFIG_HOME", origXDGConfigHome)

	pdir := filepath.Join(home, ".config", "symbrain", "profiles")
	if err := os.MkdirAll(pdir, 0755); err != nil {
		return ProfileExpectation{ID: tc.ID, Success: false, ErrorSubstr: err.Error()}
	}

	filePath := filepath.Join(pdir, tc.Name+".toml")
	if err := os.MkdirAll(filepath.Dir(filePath), 0755); err != nil {
		return ProfileExpectation{ID: tc.ID, Success: false, ErrorSubstr: err.Error()}
	}
	if err := os.WriteFile(filePath, []byte(tc.TOML), 0644); err != nil {
		return ProfileExpectation{ID: tc.ID, Success: false, ErrorSubstr: err.Error()}
	}

	p, err := profile.Load(tc.Name)
	if err != nil {
		return ProfileExpectation{
			ID:          tc.ID,
			InputName:   tc.Name,
			InputTOML:   tc.TOML,
			Success:     false,
			ErrorSubstr: stableProfileError(tc.Name, err.Error()),
		}
	}

	servers := make(map[string]ExpectedServer, len(p.Servers))
	for alias, s := range p.Servers {
		servers[alias] = ExpectedServer{
			Enabled:    s.Enabled,
			Mode:       s.Mode,
			Command:    s.Command,
			Args:       s.Args,
			URL:        s.URL,
			Access:     s.Access,
			ToolsAllow: s.ToolsAllow,
			ToolsDeny:  s.ToolsDeny,
			ToolsRead:  s.ToolsRead,
			ToolsWrite: s.ToolsWrite,
		}
	}

	return ProfileExpectation{
		ID:          tc.ID,
		InputName:   tc.Name,
		InputTOML:   tc.TOML,
		Success:     true,
		Name:        p.Name,
		Description: p.Description,
		Audit:       p.Audit.Enabled,
		Servers:     servers,
		Warnings:    p.Warnings,
	}
}

func stableProfileError(name, message string) string {
	prefix := fmt.Sprintf("profile %q: failed to parse TOML", name)
	if strings.Contains(message, prefix) {
		return prefix
	}
	return message
}

func runPolicyCase(tc PolicyTestCase) PolicyExpectation {
	cfg := profile.ServerConfig{
		Enabled:    tc.Config.Enabled,
		Mode:       tc.Config.Mode,
		Command:    tc.Config.Command,
		Args:       tc.Config.Args,
		URL:        tc.Config.URL,
		Access:     tc.Config.Access,
		ToolsAllow: tc.Config.ToolsAllow,
		ToolsDeny:  tc.Config.ToolsDeny,
		ToolsRead:  tc.Config.ToolsRead,
		ToolsWrite: tc.Config.ToolsWrite,
	}

	var rep *policy.Report
	var err error

	if tc.PresetEval {
		rep, err = policy.EvaluatePreset(tc.Server, cfg)
	} else if tc.IsForeign || len(tc.ForeignTools) > 0 {
		tools := make([]policy.ForeignTool, len(tc.ForeignTools))
		for i, ft := range tc.ForeignTools {
			tools[i] = policy.ForeignTool{Name: ft.Name, ReadOnlyHint: ft.ReadOnlyHint}
		}
		rep, err = policy.EvaluateForeign(tc.Server, cfg, tools)
	} else {
		rep, err = policy.Evaluate(tc.Server, cfg, tc.LiveTools)
	}

	if err != nil {
		return PolicyExpectation{
			ID:                tc.ID,
			InputServer:       tc.Server,
			InputConfig:       tc.Config,
			InputLiveTools:    tc.LiveTools,
			InputForeignTools: tc.ForeignTools,
			PresetEval:        tc.PresetEval,
			IsForeign:         tc.IsForeign,
			Success:           false,
			ErrorSubstr:       err.Error(),
		}
	}

	var exposures map[string]ToolExposureExpectation
	if rep.Exposures != nil {
		exposures = make(map[string]ToolExposureExpectation, len(rep.Exposures))
		for k, v := range rep.Exposures {
			exposures[k] = ToolExposureExpectation{Class: v.Class, Source: v.Source}
		}
	}

	return PolicyExpectation{
		ID:                tc.ID,
		InputServer:       tc.Server,
		InputConfig:       tc.Config,
		InputLiveTools:    tc.LiveTools,
		InputForeignTools: tc.ForeignTools,
		PresetEval:        tc.PresetEval,
		IsForeign:         tc.IsForeign,
		Success:           true,
		Server:            rep.Server,
		Enabled:           rep.Enabled,
		Mode:              rep.Mode,
		Exposed:           rep.Exposed,
		Hidden:            rep.Hidden,
		Unknown:           rep.Unknown,
		Exposures:         exposures,
	}
}

func main() {
	check := flag.Bool("check", false, "fail if generated output does not match existing file")
	output := flag.String("output", "rust/symbrain-policy/tests/fixtures/oracle_expectations.json", "output expectations path")
	flag.Parse()

	suite := OracleExpectations{}
	for _, tc := range getProfileTestCases() {
		suite.ProfileCases = append(suite.ProfileCases, runProfileCase(tc))
	}
	for _, tc := range getPolicyTestCases() {
		suite.PolicyCases = append(suite.PolicyCases, runPolicyCase(tc))
	}

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
			fmt.Fprintf(os.Stderr, "%s is out of date; run go run ./scripts/policy-oracle\n", *output)
			os.Exit(1)
		}
		fmt.Printf("PASS: policy oracle deterministic check passed (0 drift on %d profile cases, %d policy cases)\n",
			len(suite.ProfileCases), len(suite.PolicyCases))
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

	fmt.Printf("Wrote %s (%d profile cases, %d policy cases)\n", *output, len(suite.ProfileCases), len(suite.PolicyCases))
}
