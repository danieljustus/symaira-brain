package main

import (
	"context"
	"encoding/json"
	"fmt"
	"log/slog"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/danieljustus/symaira-browse/internal/daemon"
)

type contract struct {
	Open                  daemon.Response `json:"open"`
	SubresourceAllowed    daemon.Response `json:"subresource_allowed"`
	SubresourcePolicy     daemon.Response `json:"subresource_policy"`
	OpenFragment          daemon.Response `json:"open_fragment"`
	OpenRelative          daemon.Response `json:"open_relative"`
	OpenBlank             daemon.Response `json:"open_blank"`
	OpenData              daemon.Response `json:"open_data"`
	ScrollIntoView        daemon.Response `json:"scroll_into_view"`
	TabNew                daemon.Response `json:"tab_new"`
	TabNewRelative        daemon.Response `json:"tab_new_relative"`
	InspectText           daemon.Response `json:"inspect_text"`
	InspectHTML           daemon.Response `json:"inspect_html"`
	InspectTitle          daemon.Response `json:"inspect_title"`
	InspectURL            daemon.Response `json:"inspect_url"`
	InspectSelectedTitle  daemon.Response `json:"inspect_selected_title"`
	InspectSelectedURL    daemon.Response `json:"inspect_selected_url"`
	InspectValue          daemon.Response `json:"inspect_value"`
	InspectHiddenText     daemon.Response `json:"inspect_hidden_text"`
	InspectBox            daemon.Response `json:"inspect_box"`
	InspectStyles         daemon.Response `json:"inspect_styles"`
	InspectStylesWanted   daemon.Response `json:"inspect_styles_wanted"`
	InspectVisible        daemon.Response `json:"inspect_visible"`
	InspectError          daemon.Response `json:"inspect_error"`
	InspectMissingElement daemon.Response `json:"inspect_missing_element"`
	ClickObstructed       daemon.Response `json:"click_obstructed"`
	PopupClick            daemon.Response `json:"popup_click"`
	PopupOpen             daemon.Response `json:"popup_open"`
	EvalException         daemon.Response `json:"eval_exception"`
	TabList               daemon.Response `json:"tab_list"`
	TabClose              daemon.Response `json:"tab_close"`
	LastTabClose          daemon.Response `json:"last_tab_close"`
	NetworkCapture        daemon.Response `json:"network_capture"`
	NetworkRequests       daemon.Response `json:"network_requests"`
	NetworkRequest        daemon.Response `json:"network_request"`
	NetworkMissingRequest daemon.Response `json:"network_missing_request"`
	RuntimeConsoleInitial daemon.Response `json:"runtime_console_initial"`
	RuntimeConsoleEmit    daemon.Response `json:"runtime_console_emit"`
	RuntimeConsoleList    daemon.Response `json:"runtime_console_list"`
	RuntimeConsoleClear   daemon.Response `json:"runtime_console_clear"`
	RuntimeConsoleCleared daemon.Response `json:"runtime_console_cleared"`
	RuntimeErrorsInitial  daemon.Response `json:"runtime_errors_initial"`
	RuntimeExceptionEmit  daemon.Response `json:"runtime_exception_emit"`
	RuntimeErrorsList     daemon.Response `json:"runtime_errors_list"`
	RuntimeErrorsClear    daemon.Response `json:"runtime_errors_clear"`
	RuntimeErrorsCleared  daemon.Response `json:"runtime_errors_cleared"`
}

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func run() error {
	slog.SetDefault(slog.New(slog.NewTextHandler(os.Stderr, &slog.HandlerOptions{Level: slog.LevelDebug})))
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
	fixture, err := url.Parse(os.Args[2])
	if err != nil || fixture.Hostname() == "" {
		return fmt.Errorf("parse fixture URL for domain policy: %q", os.Args[2])
	}
	runtime := daemon.NewNavigationRuntime(registry, os.Args[1], daemon.NavigationRuntimeOptions{
		Headless:       true,
		AllowedDomains: []string{fixture.Hostname()},
		RequestTimeout: 45 * time.Second,
	})
	defer func() { _ = runtime.Close() }()

	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	defer cancel()

	var result contract
	result.Open = call(runtime, ctx, daemon.Frame{
		Cmd:     "open",
		Session: "chrome-contract",
		Args:    mustJSON(map[string]string{"url": os.Args[2] + "/page"}),
	})
	if !result.Open.Success {
		return fmt.Errorf("Go open oracle failed: %s", responseJSON(result.Open))
	}
	controlProbe := fmt.Sprintf("Promise.race([fetch('%s/policy-pixel').then(() => 'loaded', () => 'blocked'), new Promise(resolve => setTimeout(() => resolve('timed_out'), 3000))])", os.Args[2])
	result.SubresourceAllowed = call(runtime, ctx, daemon.Frame{
		Cmd: "eval", Session: "chrome-contract", Args: mustJSON(map[string]string{"expression": controlProbe}),
	})
	if !result.SubresourceAllowed.Success || responseValue(result.SubresourceAllowed) != "loaded" {
		return fmt.Errorf("Go allowed subresource control failed: %s", responseJSON(result.SubresourceAllowed))
	}
	policyProbe := fmt.Sprintf("Promise.race([fetch('http://localhost:%s/policy-pixel').then(() => 'loaded', () => 'blocked'), new Promise(resolve => setTimeout(() => resolve('timed_out'), 3000))])", fixture.Port())
	result.SubresourcePolicy = call(runtime, ctx, daemon.Frame{
		Cmd: "eval", Session: "chrome-contract", Args: mustJSON(map[string]string{"expression": policyProbe}),
	})
	if !result.SubresourcePolicy.Success || result.SubresourcePolicy.Data == nil {
		return fmt.Errorf("Go subresource policy oracle failed: %s", responseJSON(result.SubresourcePolicy))
	}
	if responseValue(result.SubresourcePolicy) != "blocked" {
		return fmt.Errorf("Go subresource policy was not blocked: %s", responseJSON(result.SubresourcePolicy))
	}
	result.OpenFragment = call(runtime, ctx, daemon.Frame{
		Cmd:     "open",
		Session: "chrome-contract",
		Args:    mustJSON(map[string]string{"url": os.Args[2] + "/page#same-document"}),
	})
	if !result.OpenFragment.Success {
		return fmt.Errorf("Go same-document open oracle failed: %s", responseJSON(result.OpenFragment))
	}
	result.OpenRelative = call(runtime, ctx, daemon.Frame{
		Cmd:     "open",
		Session: "chrome-contract",
		Args:    mustJSON(map[string]string{"url": "relative-probe"}),
	})
	if result.OpenRelative.Success {
		return fmt.Errorf("Go relative open unexpectedly succeeded: %s", responseJSON(result.OpenRelative))
	}
	// Go enables capture on the first network.requests call for an existing tab.
	result.NetworkCapture = call(runtime, ctx, daemon.Frame{
		Cmd:     "network.requests",
		Session: "chrome-contract",
	})
	if !result.NetworkCapture.Success {
		return fmt.Errorf("Go network.requests capture start failed: %s", responseJSON(result.NetworkCapture))
	}
	result.ScrollIntoView = call(runtime, ctx, daemon.Frame{
		Cmd:     "scrollintoview",
		Session: "chrome-contract",
		Args:    mustJSON(map[string]string{"selector": "#target"}),
	})
	if !result.ScrollIntoView.Success {
		return fmt.Errorf("Go scrollintoview oracle failed: %s", responseJSON(result.ScrollIntoView))
	}
	reloaded := call(runtime, ctx, daemon.Frame{Cmd: "reload", Session: "chrome-contract"})
	if !reloaded.Success {
		return fmt.Errorf("Go reload after capture start failed: %s", responseJSON(reloaded))
	}
	result.NetworkRequests = call(runtime, ctx, daemon.Frame{
		Cmd:     "network.requests",
		Session: "chrome-contract",
	})
	if !result.NetworkRequests.Success {
		return fmt.Errorf("Go network.requests oracle failed: %s", responseJSON(result.NetworkRequests))
	}
	var requests struct {
		Requests []struct {
			ID  string `json:"id"`
			URL string `json:"url"`
		} `json:"requests"`
	}
	requestBytes, err := json.Marshal(result.NetworkRequests.Data)
	if err != nil {
		return fmt.Errorf("encode Go network.requests data: %w", err)
	}
	if err := json.Unmarshal(requestBytes, &requests); err != nil {
		return fmt.Errorf("decode Go network.requests data: %w", err)
	}
	var pageRequestID string
	for _, request := range requests.Requests {
		if request.URL == os.Args[2]+"/page" {
			pageRequestID = request.ID
			break
		}
	}
	if pageRequestID == "" {
		return fmt.Errorf("Go network.requests did not capture %q", os.Args[2]+"/page")
	}
	result.NetworkRequest = call(runtime, ctx, daemon.Frame{
		Cmd:     "network.request",
		Session: "chrome-contract",
		Args:    mustJSON(map[string]string{"id": pageRequestID}),
	})
	if !result.NetworkRequest.Success {
		return fmt.Errorf("Go network.request oracle failed: %s", responseJSON(result.NetworkRequest))
	}
	result.NetworkMissingRequest = call(runtime, ctx, daemon.Frame{
		Cmd:     "network.request",
		Session: "chrome-contract",
		Args:    mustJSON(map[string]string{"id": "missing-network-request"}),
	})
	if result.NetworkMissingRequest.Success || result.NetworkMissingRequest.Error == nil || result.NetworkMissingRequest.Error.Code != "network_request_not_found" {
		return fmt.Errorf("Go missing network.request oracle was unexpected: %s", responseJSON(result.NetworkMissingRequest))
	}
	result.RuntimeConsoleInitial = call(runtime, ctx, daemon.Frame{
		Cmd: "console.list", Session: "chrome-contract",
	})
	if !result.RuntimeConsoleInitial.Success || runtimeEntryCount(result.RuntimeConsoleInitial) != 0 {
		return fmt.Errorf("Go initial console.list oracle was unexpected: %s", responseJSON(result.RuntimeConsoleInitial))
	}
	result.RuntimeConsoleEmit = call(runtime, ctx, daemon.Frame{
		Cmd: "eval", Session: "chrome-contract",
		Args: mustJSON(map[string]string{"expression": "console.warn('symbrowse runtime console probe')"}),
	})
	if !result.RuntimeConsoleEmit.Success {
		return fmt.Errorf("Go console event script failed: %s", responseJSON(result.RuntimeConsoleEmit))
	}
	result.RuntimeConsoleList, err = waitRuntimeEntryCount(runtime, ctx, "console.list", 1)
	if err != nil {
		return err
	}
	if !runtimeEntriesContain(result.RuntimeConsoleList, "symbrowse runtime console probe") {
		return fmt.Errorf("Go console.list omitted emitted probe: %s", responseJSON(result.RuntimeConsoleList))
	}
	result.RuntimeConsoleClear = call(runtime, ctx, daemon.Frame{
		Cmd: "console.clear", Session: "chrome-contract",
	})
	result.RuntimeConsoleCleared = call(runtime, ctx, daemon.Frame{
		Cmd: "console.list", Session: "chrome-contract",
	})
	if !result.RuntimeConsoleClear.Success || runtimeEntryCount(result.RuntimeConsoleCleared) != 0 {
		return fmt.Errorf("Go console.clear oracle was unexpected: clear=%s list=%s", responseJSON(result.RuntimeConsoleClear), responseJSON(result.RuntimeConsoleCleared))
	}
	result.RuntimeErrorsInitial = call(runtime, ctx, daemon.Frame{
		Cmd: "errors.list", Session: "chrome-contract",
	})
	if !result.RuntimeErrorsInitial.Success || runtimeEntryCount(result.RuntimeErrorsInitial) != 0 {
		return fmt.Errorf("Go initial errors.list oracle was unexpected: %s", responseJSON(result.RuntimeErrorsInitial))
	}
	result.RuntimeExceptionEmit = call(runtime, ctx, daemon.Frame{
		Cmd: "eval", Session: "chrome-contract",
		Args: mustJSON(map[string]string{"expression": "setTimeout(() => { throw new Error('symbrowse uncaught runtime probe') }, 0)"}),
	})
	if !result.RuntimeExceptionEmit.Success {
		return fmt.Errorf("Go uncaught exception script failed: %s", responseJSON(result.RuntimeExceptionEmit))
	}
	result.RuntimeErrorsList, err = waitRuntimeEntryCount(runtime, ctx, "errors.list", 1)
	if err != nil {
		return err
	}
	result.RuntimeErrorsClear = call(runtime, ctx, daemon.Frame{
		Cmd: "errors.clear", Session: "chrome-contract",
	})
	result.RuntimeErrorsCleared = call(runtime, ctx, daemon.Frame{
		Cmd: "errors.list", Session: "chrome-contract",
	})
	if !result.RuntimeErrorsClear.Success || runtimeEntryCount(result.RuntimeErrorsCleared) != 0 {
		return fmt.Errorf("Go errors.clear oracle was unexpected: clear=%s list=%s", responseJSON(result.RuntimeErrorsClear), responseJSON(result.RuntimeErrorsCleared))
	}
	result.TabNew = call(runtime, ctx, daemon.Frame{
		Cmd:     "tab.new",
		Session: "chrome-contract",
		Args:    mustJSON(map[string]string{"label": "second", "url": os.Args[2] + "/popup"}),
	})
	if !result.TabNew.Success {
		return fmt.Errorf("Go tab.new oracle failed: %s", responseJSON(result.TabNew))
	}
	result.TabNewRelative = call(runtime, ctx, daemon.Frame{
		Cmd:     "tab.new",
		Session: "chrome-contract",
		Args:    mustJSON(map[string]string{"url": "relative-probe"}),
	})
	if result.TabNewRelative.Success {
		return fmt.Errorf("Go relative tab.new unexpectedly succeeded: %s", responseJSON(result.TabNewRelative))
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
	result.InspectValue = call(runtime, ctx, daemon.Frame{
		Cmd:     "get.value",
		Session: "chrome-contract",
		Args:    mustJSON(map[string]string{"selector": "#plain"}),
	})
	result.InspectHiddenText = call(runtime, ctx, daemon.Frame{
		Cmd:     "get.text",
		Session: "chrome-contract",
		Args:    mustJSON(map[string]string{"selector": "#hidden"}),
	})
	result.InspectBox = call(runtime, ctx, daemon.Frame{
		Cmd:     "get.box",
		Session: "chrome-contract",
		Args:    mustJSON(map[string]string{"selector": "#popup"}),
	})
	result.InspectStyles = call(runtime, ctx, daemon.Frame{
		Cmd:     "get.styles",
		Session: "chrome-contract",
		Args:    mustJSON(map[string]string{"selector": "#popup"}),
	})
	result.InspectStylesWanted = call(runtime, ctx, daemon.Frame{
		Cmd:     "get.styles",
		Session: "chrome-contract",
		Args: mustJSON(map[string]any{
			"selector":   "#popup",
			"properties": []string{"display", "visibility", "color"},
		}),
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
		{"get.value", result.InspectValue},
		{"get.text hidden", result.InspectHiddenText},
		{"get.box", result.InspectBox},
		{"get.styles", result.InspectStyles},
		{"get.styles properties", result.InspectStylesWanted},
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
	result.ClickObstructed = call(runtime, ctx, daemon.Frame{
		Cmd:     "click",
		Session: "chrome-contract",
		Args:    mustJSON(map[string]string{"selector": "#covered"}),
	})
	if result.ClickObstructed.Success || result.ClickObstructed.Error == nil || result.ClickObstructed.Error.Code != "click_obstructed" {
		return fmt.Errorf("Go click obstruction oracle was unexpected: %s", responseJSON(result.ClickObstructed))
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
	result.EvalException = call(runtime, ctx, daemon.Frame{
		Cmd:     "eval",
		Session: "chrome-contract",
		Args:    mustJSON(map[string]string{"expression": "(() => { throw new Error('native eval failure') })()"}),
	})
	if !result.EvalException.Success {
		return fmt.Errorf("Go eval exception oracle failed: %s", responseJSON(result.EvalException))
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
	result.OpenBlank = call(runtime, ctx, daemon.Frame{
		Cmd:     "open",
		Session: "chrome-contract",
		Args:    mustJSON(map[string]string{"url": "about:blank"}),
	})
	if result.OpenBlank.Success {
		return fmt.Errorf("Go about:blank open unexpectedly succeeded: %s", responseJSON(result.OpenBlank))
	}
	result.OpenData = call(runtime, ctx, daemon.Frame{
		Cmd:     "open",
		Session: "chrome-contract",
		Args:    mustJSON(map[string]string{"url": "data:text/html,<title>daemon</title><h1>native</h1><div style='height:12000px'><div id='target' style='margin-top:8000px;height:100px'></div></div>"}),
	})
	if result.OpenData.Success {
		return fmt.Errorf("Go data: open unexpectedly succeeded: %s", responseJSON(result.OpenData))
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

func responseValue(response daemon.Response) string {
	raw, err := json.Marshal(response.Data)
	if err != nil {
		return ""
	}
	var data struct {
		Value string `json:"value"`
	}
	if json.Unmarshal(raw, &data) != nil {
		return ""
	}
	return data.Value
}

func waitRuntimeEntryCount(runtime *daemon.NavigationRuntime, ctx context.Context, command string, want int) (daemon.Response, error) {
	deadline := time.Now().Add(5 * time.Second)
	var last daemon.Response
	for time.Now().Before(deadline) {
		last = call(runtime, ctx, daemon.Frame{Cmd: command, Session: "chrome-contract"})
		if !last.Success {
			return last, fmt.Errorf("Go %s failed while waiting for event: %s", command, responseJSON(last))
		}
		if runtimeEntryCount(last) >= want {
			return last, nil
		}
		select {
		case <-ctx.Done():
			return last, fmt.Errorf("Go %s timed out while waiting for runtime event: %w", command, ctx.Err())
		case <-time.After(10 * time.Millisecond):
		}
	}
	return last, fmt.Errorf("Go %s did not capture %d runtime entries: %s", command, want, responseJSON(last))
}

func runtimeEntryCount(response daemon.Response) int {
	return len(runtimeEntries(response))
}

func runtimeEntriesContain(response daemon.Response, text string) bool {
	for _, raw := range runtimeEntries(response) {
		var entry struct {
			Text string `json:"text"`
		}
		if json.Unmarshal(raw, &entry) == nil && strings.Contains(entry.Text, text) {
			return true
		}
	}
	return false
}

func runtimeEntries(response daemon.Response) []json.RawMessage {
	encoded, err := json.Marshal(response.Data)
	if err != nil {
		return nil
	}
	var data struct {
		Entries []json.RawMessage `json:"entries"`
	}
	if json.Unmarshal(encoded, &data) != nil {
		return nil
	}
	return data.Entries
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
