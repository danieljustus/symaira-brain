//go:build ignore

// Command session_registry_oracle emits the stable Go session-registry contract.
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/danieljustus/symaira-browse/internal/daemon"
)

type sessionView struct {
	Name                   string `json:"name"`
	PID                    int    `json:"pid"`
	ActiveTabs             int    `json:"active_tabs"`
	UserDataDirBasename    string `json:"user_data_dir_basename"`
	UserDataPathIsClean    bool   `json:"user_data_path_is_clean"`
	BrowserContextID       string `json:"browser_context_id"`
	RefCount               int    `json:"ref_count"`
	Scope                  string `json:"scope"`
	OriginPath             string `json:"origin_path"`
	TimestampFormatValid   bool   `json:"timestamp_format_valid"`
	LastActivityNotEarlier bool   `json:"last_activity_not_earlier"`
}

type output struct {
	SchemaVersion      int            `json:"schema_version"`
	DefaultProfileRoot string         `json:"default_profile_root"`
	Sessions           []sessionView  `json:"sessions"`
	InvalidNameError   string         `json:"invalid_name_error"`
	MissingGetError    string         `json:"missing_get_error"`
	MissingTouchError  string         `json:"missing_touch_error"`
	EmptyRefError      string         `json:"empty_ref_error"`
	Reference          string         `json:"reference"`
	EnsureIdempotent   bool           `json:"ensure_idempotent"`
	Validation         []idValidation `json:"validation"`
	TouchAdvanced      bool           `json:"touch_advanced"`
	ClearedEntries     int            `json:"cleared_entries"`
	ProfilesPreserved  bool           `json:"profiles_preserved"`
}

type idValidation struct {
	Name     string `json:"name"`
	Accepted bool   `json:"accepted"`
}

func view(info daemon.SessionInfo) sessionView {
	started, startedErr := time.Parse(time.RFC3339Nano, info.StartedAt)
	activity, activityErr := time.Parse(time.RFC3339Nano, info.LastActivity)
	return sessionView{
		Name:                   info.Name,
		PID:                    info.PID,
		ActiveTabs:             info.ActiveTabs,
		UserDataDirBasename:    filepath.Base(info.UserDataDir),
		UserDataPathIsClean:    filepath.Clean(info.UserDataDir) == info.UserDataDir,
		BrowserContextID:       info.BrowserContextID,
		RefCount:               info.RefCount,
		Scope:                  info.Scope,
		OriginPath:             info.OriginPath,
		TimestampFormatValid:   startedErr == nil && activityErr == nil,
		LastActivityNotEarlier: startedErr == nil && activityErr == nil && !activity.Before(started),
	}
}

func main() {
	defaultRegistry := daemon.NewSessionRegistry(daemon.SessionRegistryOptions{})
	root, err := os.MkdirTemp("", "symbrowse-session-oracle-")
	if err != nil {
		panic(err)
	}
	defer os.RemoveAll(root)
	uncleanRoot := root + string(os.PathSeparator) + "unused" + string(os.PathSeparator) + ".."
	registry := daemon.NewSessionRegistry(daemon.SessionRegistryOptions{
		PID: 4242, UserDataRoot: uncleanRoot, Scope: "worktree", OriginPath: "/workspace/project",
	})
	_, invalidName := registry.Ensure("../escape")
	if invalidName == nil {
		panic("invalid session name was accepted")
	}
	if _, err := registry.Ensure("beta"); err != nil {
		panic(err)
	}
	alpha, err := registry.Ensure("alpha")
	if err != nil {
		panic(err)
	}
	again, err := registry.Ensure("alpha")
	if err != nil {
		panic(err)
	}
	if err := registry.SetActiveTabs("alpha", 3); err != nil {
		panic(err)
	}
	if err := registry.SetRef("alpha", "selector", "@element-1"); err != nil {
		panic(err)
	}
	ref, err := registry.Ref("alpha", "selector")
	if err != nil {
		panic(err)
	}
	beforeTouch, err := registry.Get("alpha")
	if err != nil {
		panic(err)
	}
	time.Sleep(2 * time.Millisecond)
	if err := registry.Touch("alpha"); err != nil {
		panic(err)
	}
	afterTouch, err := registry.Get("alpha")
	if err != nil {
		panic(err)
	}

	validationRegistry := daemon.NewSessionRegistry(daemon.SessionRegistryOptions{UserDataRoot: filepath.Join(root, "validation")})
	validation := make([]idValidation, 0, 7)
	for _, name := range []string{"", strings.Repeat("x", 65), "../escape", "ümlaut", "has space"} {
		_, err := validationRegistry.Ensure(name)
		validation = append(validation, idValidation{Name: name, Accepted: err == nil})
	}
	for _, name := range []string{"a", strings.Repeat("x", 64)} {
		_, err := validationRegistry.Ensure(name)
		validation = append(validation, idValidation{Name: name, Accepted: err == nil})
	}
	data := registry.ListData()
	views := make([]sessionView, 0, len(data.Sessions))
	for _, info := range data.Sessions {
		views = append(views, view(info))
	}
	_, missingGet := registry.Get("missing")
	missingTouch := registry.Touch("missing")
	emptyRef := registry.SetRef("alpha", "", "value")
	registry.Clear()
	profileExists := true
	for _, name := range []string{"alpha", "beta"} {
		if _, err := os.Stat(filepath.Join(root, name)); err != nil {
			profileExists = false
		}
	}
	if !profileExists {
		panic("session registry clear removed profile directories")
	}
	result := output{
		SchemaVersion: data.SchemaVersion, DefaultProfileRoot: defaultRegistry.UserDataRoot(), Sessions: views,
		InvalidNameError: invalidName.Error(),
		MissingGetError:  missingGet.Error(), MissingTouchError: missingTouch.Error(),
		EmptyRefError: emptyRef.Error(), Reference: ref, EnsureIdempotent: again == alpha,
		Validation: validation, TouchAdvanced: afterTouch.LastActivity != beforeTouch.LastActivity,
		ClearedEntries: len(registry.List()), ProfilesPreserved: profileExists,
	}
	if err := json.NewEncoder(os.Stdout).Encode(result); err != nil {
		panic(fmt.Errorf("encode oracle: %w", err))
	}
}
