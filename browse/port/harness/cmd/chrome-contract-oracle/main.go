package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"time"

	"github.com/danieljustus/symaira-browse/internal/daemon"
)

type contract struct {
	Open                  daemon.Response `json:"open"`
	TabNew                daemon.Response `json:"tab_new"`
	InspectText           daemon.Response `json:"inspect_text"`
	InspectHTML           daemon.Response `json:"inspect_html"`
	InspectTitle          daemon.Response `json:"inspect_title"`
	InspectURL            daemon.Response `json:"inspect_url"`
	InspectSelectedTitle  daemon.Response `json:"inspect_selected_title"`
	InspectSelectedURL    daemon.Response `json:"inspect_selected_url"`
	InspectVisible        daemon.Response `json:"inspect_visible"`
	InspectError          daemon.Response `json:"inspect_error"`
	InspectMissingElement daemon.Response `json:"inspect_missing_element"`
	PopupClick            daemon.Response `json:"popup_click"`
	PopupOpen             daemon.Response `json:"popup_open"`
	TabList               daemon.Response `json:"tab_list"`
	TabClose              daemon.Response `json:"tab_close"`
	LastTabClose          daemon.Response `json:"last_tab_close"`
}

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func run() error {
	if len(os.Args) != 3 || os.Args[1] == "" || os.Args[2] == "" {
		return fmt.Errorf("usage: chrome-contract-oracle CHROME_EXECUTABLE FIXTURE_BASE_URL")
	}
	root, err := os.MkdirTemp("", "symbrowse-go-chrome-contract-")
	if err != nil {
		return fmt.Errorf("create isolated profile root: %w", err)
	}
	defer func() { _ = os.RemoveAll(root) }()

	registry := daemon.NewSessionRegistry(daemon.SessionRegistryOptions{
		UserDataRoot: filepath.Join(root, "profiles"),
	})
	if _, err := registry.Ensure("chrome-contract"); err != nil {
		return fmt.Errorf("ensure oracle session: %w", err)
	}
	runtime := daemon.NewNavigationRuntime(registry, os.Args[1], daemon.NavigationRuntimeOptions{
		Headless:       true,
		RequestTimeout: 45 * time.Second,
	})
	defer func() { _ = runtime.Close() }()

	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	defer cancel()

	result := contract{
		Open: call(runtime, ctx, daemon.Frame{
			Cmd:     "open",
			Session: "chrome-contract",
			Args:    mustJSON(map[string]string{"url": os.Args[2] + "/page"}),
		}),
		TabNew: call(runtime, ctx, daemon.Frame{
			Cmd:     "tab.new",
			Session: "chrome-contract",
			Args:    mustJSON(map[string]string{"label": "second", "url": os.Args[2] + "/popup"}),
		}),
	}
	if !result.Open.Success {
		return fmt.Errorf("Go open oracle failed: %s", responseJSON(result.Open))
	}
	if !result.TabNew.Success {
		return fmt.Errorf("Go tab.new oracle failed: %s", responseJSON(result.TabNew))
	}
	result.InspectText = call(runtime, ctx, daemon.Frame{
		Cmd:     "get.text",
		Session: "chrome-contract",
		Args:    mustJSON(map[string]string{"selector": "#popup"}),
	})
	if !result.InspectText.Success {
		return fmt.Errorf("Go get.text oracle failed: %s", responseJSON(result.InspectText))
	}
	result.InspectHTML = call(runtime, ctx, daemon.Frame{
		Cmd:     "get.html",
		Session: "chrome-contract",
		Args:    mustJSON(map[string]string{}),
	})
	result.InspectTitle = call(runtime, ctx, daemon.Frame{
		Cmd:     "get.title",
		Session: "chrome-contract",
		Args:    mustJSON(map[string]string{}),
	})
	result.InspectURL = call(runtime, ctx, daemon.Frame{
		Cmd:     "get.url",
		Session: "chrome-contract",
		Args:    mustJSON(map[string]string{}),
	})
	result.InspectSelectedTitle = call(runtime, ctx, daemon.Frame{
		Cmd:     "get.title",
		Session: "chrome-contract",
		Args:    mustJSON(map[string]string{"selector": "#popup"}),
	})
	result.InspectSelectedURL = call(runtime, ctx, daemon.Frame{
		Cmd:     "get.url",
		Session: "chrome-contract",
		Args:    mustJSON(map[string]string{"selector": "#link"}),
	})
	result.InspectVisible = call(runtime, ctx, daemon.Frame{
		Cmd:     "is.visible",
		Session: "chrome-contract",
		Args:    mustJSON(map[string]string{"selector": "#popup"}),
	})
	for _, inspection := range []struct {
		command  string
		response daemon.Response
	}{
		{"get.html", result.InspectHTML},
		{"get.title", result.InspectTitle},
		{"get.url", result.InspectURL},
		{"get.title selector", result.InspectSelectedTitle},
		{"get.url selector", result.InspectSelectedURL},
		{"is.visible", result.InspectVisible},
	} {
		if !inspection.response.Success {
			return fmt.Errorf("Go %s oracle failed: %s", inspection.command, responseJSON(inspection.response))
		}
	}
	result.InspectError = call(runtime, ctx, daemon.Frame{
		Cmd:     "get.text",
		Session: "chrome-contract",
		Args:    mustJSON(map[string]string{}),
	})
	if result.InspectError.Success {
		return fmt.Errorf("Go get.text without selector unexpectedly succeeded")
	}
	result.InspectMissingElement = call(runtime, ctx, daemon.Frame{
		Cmd:     "get.title",
		Session: "chrome-contract",
		Args:    mustJSON(map[string]string{"selector": "#missing"}),
	})
	if result.InspectMissingElement.Success {
		return fmt.Errorf("Go get.title for a missing element unexpectedly succeeded")
	}
	result.PopupClick = call(runtime, ctx, daemon.Frame{
		Cmd:     "click",
		Session: "chrome-contract",
		Args:    mustJSON(map[string]string{"selector": "#popup"}),
	})
	if !result.PopupClick.Success {
		return fmt.Errorf("Go popup click oracle failed: %s", responseJSON(result.PopupClick))
	}
	result.PopupOpen = call(runtime, ctx, daemon.Frame{
		Cmd:     "eval",
		Session: "chrome-contract",
		Args:    mustJSON(map[string]string{"expression": "Boolean(window.popup && !window.popup.closed)"}),
	})
	if !result.PopupOpen.Success {
		return fmt.Errorf("Go popup verification oracle failed: %s", responseJSON(result.PopupOpen))
	}
	result.TabList = call(runtime, ctx, daemon.Frame{
		Cmd:     "tab.list",
		Session: "chrome-contract",
	})
	if !result.TabList.Success {
		return fmt.Errorf("Go tab.list oracle failed: %s", responseJSON(result.TabList))
	}
	result.TabClose = call(runtime, ctx, daemon.Frame{
		Cmd:     "tab.close",
		Session: "chrome-contract",
		Args:    json.RawMessage(`{"tab":"second"}`),
	})
	if !result.TabClose.Success {
		return fmt.Errorf("Go tab.close oracle failed: %s", responseJSON(result.TabClose))
	}
	result.LastTabClose = call(runtime, ctx, daemon.Frame{
		Cmd:     "tab.close",
		Session: "chrome-contract",
	})
	if result.LastTabClose.Success {
		return fmt.Errorf("Go last-tab close unexpectedly succeeded")
	}
	return json.NewEncoder(os.Stdout).Encode(result)
}

func call(runtime *daemon.NavigationRuntime, ctx context.Context, frame daemon.Frame) daemon.Response {
	data, warnings, err := runtime.Handle(ctx, frame)
	if err != nil {
		if protocolError, ok := err.(*daemon.Error); ok {
			return daemon.Response{Success: false, Error: protocolError, Warnings: warnings}
		}
		return daemon.Response{
			Success:  false,
			Error:    daemon.NewError(daemon.ErrorOperationFailed, err.Error()),
			Warnings: warnings,
		}
	}
	return daemon.SuccessResponse(data, warnings)
}

func responseJSON(response daemon.Response) string {
	raw, _ := json.Marshal(response)
	return string(raw)
}

func mustJSON(value any) json.RawMessage {
	raw, err := json.Marshal(value)
	if err != nil {
		panic(err)
	}
	return raw
}
