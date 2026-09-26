package safari

import (
	"context"
	"fmt"
	"strconv"
	"strings"

	"github.com/danieljustus/symaira-browse/internal/engine"
)

// TabList returns the open tabs of the live Safari session. It reports the tab
// name (used as the pinned-tab reference) and current URL for each tab so the
// human can choose which tab to pin.
func (e *Engine) TabList(_ context.Context, _ engine.Context) ([]engine.TabInfo, error) {
	e.mu.Lock()
	closed := e.closed
	e.mu.Unlock()
	if closed {
		return nil, fmt.Errorf("safari engine: engine is closed")
	}
	// AppleScript returns the tab name and URL per window; we flatten to a
	// stable, machine-readable list. The live session is single-window for the
	// human, so we read window 1.
	script := `tell application "Safari"
	  set out to ""
	  repeat with t in tabs of window 1
	    set out to out & (name of t) & "\t" & (URL of t) & "\n"
	  end repeat
	  return out
	end tell`
	out, err := e.runner.Run(context.Background(), script)
	if err != nil {
		return nil, err
	}
	var tabs []engine.TabInfo
	for _, line := range strings.Split(strings.TrimRight(out, "\n"), "\n") {
		if line == "" {
			continue
		}
		parts := strings.SplitN(line, "\t", 2)
		info := engine.TabInfo{Label: parts[0]}
		if len(parts) == 2 {
			info.URL = parts[1]
		}
		tabs = append(tabs, info)
	}
	return tabs, nil
}

// TabNew opens a tab in window 1 and pins subsequent operations to its window
// and index. Safari's tab name is read-only, so a caller label cannot name it.
func (e *Engine) TabNew(ctx context.Context, _ engine.Context, _ string, url string) (engine.Page, error) {
	e.mu.Lock()
	closed := e.closed
	e.mu.Unlock()
	if closed {
		return engine.Page{}, fmt.Errorf("safari engine: engine is closed")
	}
	if err := e.guardTarget(url); err != nil {
		return engine.Page{}, err
	}
	tell := fmt.Sprintf(`tell application "Safari"
	  set targetWindow to window 1
	  make new tab with properties {URL:%q} at end of tabs of targetWindow
	  return (id of targetWindow as text) & "\t" & (count of tabs of targetWindow as text)
	end tell`, url)
	out, err := e.runner.Run(ctx, tell)
	if err != nil {
		return engine.Page{}, err
	}
	parts := strings.Split(strings.TrimSpace(out), "\t")
	if len(parts) != 2 {
		return engine.Page{}, fmt.Errorf("safari engine: invalid new-tab reference %q", out)
	}
	windowID, windowErr := strconv.Atoi(parts[0])
	tabIndex, tabErr := strconv.Atoi(parts[1])
	if windowErr != nil || tabErr != nil || windowID <= 0 || tabIndex <= 0 {
		return engine.Page{}, fmt.Errorf("safari engine: invalid new-tab reference %q", out)
	}
	e.mu.Lock()
	e.pinnedWindowID = windowID
	e.pinnedTabIndex = tabIndex
	e.pinClosed = false
	e.mu.Unlock()
	return engine.Page{ID: "safari-live"}, nil
}

// TabClose closes the pinned tab. It refuses to close the human's only tab
// without an explicit target, to avoid destroying the live session state.
func (e *Engine) TabClose(_ context.Context, _ engine.Page) error {
	e.mu.Lock()
	closed := e.closed
	pinClosed := e.pinClosed
	tabRef := e.pinnedTabRef()
	e.mu.Unlock()
	if closed {
		return fmt.Errorf("safari engine: engine is closed")
	}
	if pinClosed {
		return fmt.Errorf("safari engine: pinned tab is closed")
	}
	script := fmt.Sprintf(`tell application "Safari"
	  close %s
	end tell`, tabRef)
	_, err := e.runner.Run(context.Background(), script)
	if err == nil {
		e.mu.Lock()
		e.pinnedWindowID = 0
		e.pinnedTabIndex = 0
		e.pinClosed = true
		e.mu.Unlock()
	}
	return err
}
