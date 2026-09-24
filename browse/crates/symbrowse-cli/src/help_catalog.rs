//! Static help output captured from the Go Cobra CLI at integrated HEAD
//! `0c9d135b83525e59728c9344644f836595e054cc` for implemented Rust paths.
//! Command execution remains on the Rust dispatcher; unsupported command paths are absent.

pub(super) fn help(command: &str, suffix: &[&str]) -> Option<&'static str> {
    let path = std::iter::once(command)
        .chain(suffix.iter().copied())
        .collect::<Vec<_>>()
        .join(" ");
    match path.as_str() {
        "frame" => Some(
            r###"Inspect the nested frame tree

Usage:
  symbrowse frame [command]

Available Commands:
  tree        Show the nested frame tree of the active tab

Flags:
  -h, --help             help for frame
      --session string   daemon session name (default "default")

Global Flags:
      --json            print the unified machine-readable output envelope (shorthand for --output json)
      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default "text")

Use "symbrowse frame [command] --help" for more information about a command.
"###,
        ),
        "frame tree" => Some(
            r###"Show the nested frame tree of the active tab

Usage:
  symbrowse frame tree [flags]

Flags:
  -h, --help   help for tree

Global Flags:
      --json             print the unified machine-readable output envelope (shorthand for --output json)
      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default "text")
      --session string   daemon session name (default "default")
"###,
        ),
        "tab" => Some(
            r###"Manage session tabs (list, new, switch, close)

Usage:
  symbrowse tab [command]

Available Commands:
  close       Close a tab (default: the active one)
  list        List tabs of the session
  new         Open a new tab, optionally at a URL
  switch      Activate a tab by id or label (refs stay valid per tab)
  window      Window operations

Flags:
  -h, --help             help for tab
      --session string   daemon session name (default "default")

Global Flags:
      --json            print the unified machine-readable output envelope (shorthand for --output json)
      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default "text")

Use "symbrowse tab [command] --help" for more information about a command.
"###,
        ),
        "tab close" => Some(
            r###"Close a tab (default: the active one)

Usage:
  symbrowse tab close [t1|label] [flags]

Flags:
  -h, --help   help for close

Global Flags:
      --json             print the unified machine-readable output envelope (shorthand for --output json)
      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default "text")
      --session string   daemon session name (default "default")
"###,
        ),
        "tab list" => Some(
            r###"List tabs of the session

Usage:
  symbrowse tab list [flags]

Flags:
  -h, --help   help for list

Global Flags:
      --json             print the unified machine-readable output envelope (shorthand for --output json)
      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default "text")
      --session string   daemon session name (default "default")
"###,
        ),
        "tab new" => Some(
            r###"Open a new tab, optionally at a URL

Usage:
  symbrowse tab new [url] [flags]

Flags:
  -h, --help           help for new
      --label string   tab label for later switching

Global Flags:
      --json             print the unified machine-readable output envelope (shorthand for --output json)
      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default "text")
      --session string   daemon session name (default "default")
"###,
        ),
        "tab switch" => Some(
            r###"Activate a tab by id or label (refs stay valid per tab)

Usage:
  symbrowse tab switch <t1|label> [flags]

Flags:
  -h, --help   help for switch

Global Flags:
      --json             print the unified machine-readable output envelope (shorthand for --output json)
      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default "text")
      --session string   daemon session name (default "default")
"###,
        ),
        "tab window" => Some(
            r###"Window operations

Usage:
  symbrowse tab window [command]

Available Commands:
  window      Open a new window (new tab in a fresh browser context)

Flags:
  -h, --help   help for window

Global Flags:
      --json             print the unified machine-readable output envelope (shorthand for --output json)
      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default "text")
      --session string   daemon session name (default "default")

Use "symbrowse tab window [command] --help" for more information about a command.
"###,
        ),
        "tab window window" => Some(
            r###"Open a new window (new tab in a fresh browser context)

Usage:
  symbrowse tab window window new [flags]

Flags:
  -h, --help   help for window

Global Flags:
      --json             print the unified machine-readable output envelope (shorthand for --output json)
      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default "text")
      --session string   daemon session name (default "default")
"###,
        ),
        "dialog" => Some(
            r###"Handle JavaScript dialogs (accept, dismiss, status, auto)

Usage:
  symbrowse dialog [command]

Available Commands:
  accept      Accept the pending dialog (text for prompt dialogs)
  auto        Configure automatic dialog handling (default: dismiss)
  dismiss     Dismiss the pending dialog
  status      Show the pending dialog state

Flags:
  -h, --help             help for dialog
      --session string   daemon session name (default "default")

Global Flags:
      --json            print the unified machine-readable output envelope (shorthand for --output json)
      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default "text")

Use "symbrowse dialog [command] --help" for more information about a command.
"###,
        ),
        "dialog accept" => Some(
            r###"Accept the pending dialog (text for prompt dialogs)

Usage:
  symbrowse dialog accept [text] [flags]

Flags:
  -h, --help   help for accept

Global Flags:
      --json             print the unified machine-readable output envelope (shorthand for --output json)
      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default "text")
      --session string   daemon session name (default "default")
"###,
        ),
        "dialog auto" => Some(
            r###"Configure automatic dialog handling (default: dismiss)

Usage:
  symbrowse dialog auto <accept|dismiss|off> [flags]

Flags:
  -h, --help   help for auto

Global Flags:
      --json             print the unified machine-readable output envelope (shorthand for --output json)
      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default "text")
      --session string   daemon session name (default "default")
"###,
        ),
        "dialog dismiss" => Some(
            r###"Dismiss the pending dialog

Usage:
  symbrowse dialog dismiss [flags]

Flags:
  -h, --help   help for dismiss

Global Flags:
      --json             print the unified machine-readable output envelope (shorthand for --output json)
      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default "text")
      --session string   daemon session name (default "default")
"###,
        ),
        "dialog status" => Some(
            r###"Show the pending dialog state

Usage:
  symbrowse dialog status [flags]

Flags:
  -h, --help   help for status

Global Flags:
      --json             print the unified machine-readable output envelope (shorthand for --output json)
      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default "text")
      --session string   daemon session name (default "default")
"###,
        ),
        "click" => Some(
            r###"click performs the click interaction on the targeted element.

Accepted selector forms:
  - CSS selector (e.g. "button.submit", "#username", "input[name='q']")
  - Stable @eN ref from snapshot (e.g. "@e1", "@e2")
  - Role/name pair as supported by the engine (e.g. role and accessible name)

Optional [value] argument:
  Not used for this interaction.

Usage:
  symbrowse click <selector> [value] [flags]

Flags:
  -h, --help             help for click
      --session string   session name (default "default")

Global Flags:
      --json            print the unified machine-readable output envelope (shorthand for --output json)
      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default "text")
"###,
        ),
        "fill" => Some(
            r###"fill performs the fill interaction on the targeted element.

Accepted selector forms:
  - CSS selector (e.g. "button.submit", "#username", "input[name='q']")
  - Stable @eN ref from snapshot (e.g. "@e1", "@e2")
  - Role/name pair as supported by the engine (e.g. role and accessible name)

Optional [value] argument:
  The text value to fill into the input, replacing any existing content.

Usage:
  symbrowse fill <selector> [value] [flags]

Flags:
  -h, --help             help for fill
      --session string   session name (default "default")

Global Flags:
      --json            print the unified machine-readable output envelope (shorthand for --output json)
      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default "text")
"###,
        ),
        "find" => Some(
            r###"Find an element semantically and optionally act on it

Usage:
  symbrowse find <role|text|label|placeholder|alt|title|testid> <query> <action> [value] [flags]

Flags:
      --exact            require exact matching
  -h, --help             help for find
      --name string      filter by accessible name
      --session string   session name (default "default")

Global Flags:
      --json            print the unified machine-readable output envelope (shorthand for --output json)
      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default "text")
"###,
        ),
        "get" => Some(
            r###"Inspect page and element values

Usage:
  symbrowse get [command]

Available Commands:
  attr        Inspect attr
  box         Inspect box
  count       Inspect count
  html        Inspect html
  styles      Inspect styles
  text        Inspect text
  title       Inspect title
  url         Inspect url
  value       Inspect value

Flags:
  -h, --help             help for get
      --max-tokens int   token budget for the payload; oversized output is truncated and stored in the cache (0 = no limit)
      --session string   session name (default "default")

Global Flags:
      --json            print the unified machine-readable output envelope (shorthand for --output json)
      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default "text")

Use "symbrowse get [command] --help" for more information about a command.
"###,
        ),
        "get attr" => Some(
            r###"Inspect attr

Usage:
  symbrowse get attr <selector> <attribute> [flags]

Flags:
  -h, --help   help for attr

Global Flags:
      --json             print the unified machine-readable output envelope (shorthand for --output json)
      --max-tokens int   token budget for the payload; oversized output is truncated and stored in the cache (0 = no limit)
      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default "text")
      --session string   session name (default "default")
"###,
        ),
        "get box" => Some(
            r###"Inspect box

Usage:
  symbrowse get box <selector> [flags]

Flags:
  -h, --help   help for box

Global Flags:
      --json             print the unified machine-readable output envelope (shorthand for --output json)
      --max-tokens int   token budget for the payload; oversized output is truncated and stored in the cache (0 = no limit)
      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default "text")
      --session string   session name (default "default")
"###,
        ),
        "get count" => Some(
            r###"Inspect count

Usage:
  symbrowse get count <selector> [flags]

Flags:
  -h, --help   help for count

Global Flags:
      --json             print the unified machine-readable output envelope (shorthand for --output json)
      --max-tokens int   token budget for the payload; oversized output is truncated and stored in the cache (0 = no limit)
      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default "text")
      --session string   session name (default "default")
"###,
        ),
        "get html" => Some(
            r###"Inspect html

Usage:
  symbrowse get html <selector> [flags]

Flags:
  -h, --help   help for html

Global Flags:
      --json             print the unified machine-readable output envelope (shorthand for --output json)
      --max-tokens int   token budget for the payload; oversized output is truncated and stored in the cache (0 = no limit)
      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default "text")
      --session string   session name (default "default")
"###,
        ),
        "get styles" => Some(
            r###"Inspect styles

Usage:
  symbrowse get styles <selector> [property...] [flags]

Flags:
  -h, --help   help for styles

Global Flags:
      --json             print the unified machine-readable output envelope (shorthand for --output json)
      --max-tokens int   token budget for the payload; oversized output is truncated and stored in the cache (0 = no limit)
      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default "text")
      --session string   session name (default "default")
"###,
        ),
        "get text" => Some(
            r###"Inspect text

Usage:
  symbrowse get text <selector> [flags]

Flags:
  -h, --help   help for text

Global Flags:
      --json             print the unified machine-readable output envelope (shorthand for --output json)
      --max-tokens int   token budget for the payload; oversized output is truncated and stored in the cache (0 = no limit)
      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default "text")
      --session string   session name (default "default")
"###,
        ),
        "get title" => Some(
            r###"Inspect title

Usage:
  symbrowse get title [selector] [flags]

Flags:
  -h, --help   help for title

Global Flags:
      --json             print the unified machine-readable output envelope (shorthand for --output json)
      --max-tokens int   token budget for the payload; oversized output is truncated and stored in the cache (0 = no limit)
      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default "text")
      --session string   session name (default "default")
"###,
        ),
        "get url" => Some(
            r###"Inspect url

Usage:
  symbrowse get url [selector] [flags]

Flags:
  -h, --help   help for url

Global Flags:
      --json             print the unified machine-readable output envelope (shorthand for --output json)
      --max-tokens int   token budget for the payload; oversized output is truncated and stored in the cache (0 = no limit)
      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default "text")
      --session string   session name (default "default")
"###,
        ),
        "get value" => Some(
            r###"Inspect value

Usage:
  symbrowse get value <selector> [flags]

Flags:
  -h, --help   help for value

Global Flags:
      --json             print the unified machine-readable output envelope (shorthand for --output json)
      --max-tokens int   token budget for the payload; oversized output is truncated and stored in the cache (0 = no limit)
      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default "text")
      --session string   session name (default "default")
"###,
        ),
        "goto" => Some(
            r###"Navigate to a URL (alias for open)

Usage:
  symbrowse goto [url] [flags]

Flags:
  -h, --help             help for goto
      --session string   session name (default "default")

Global Flags:
      --json            print the unified machine-readable output envelope (shorthand for --output json)
      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default "text")
"###,
        ),
        "is" => Some(
            r###"Check page and element state

Usage:
  symbrowse is [command]

Available Commands:
  checked     Inspect checked
  enabled     Inspect enabled
  visible     Inspect visible

Flags:
  -h, --help             help for is
      --session string   session name (default "default")

Global Flags:
      --json            print the unified machine-readable output envelope (shorthand for --output json)
      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default "text")

Use "symbrowse is [command] --help" for more information about a command.
"###,
        ),
        "is checked" => Some(
            r###"Inspect checked

Usage:
  symbrowse is checked <selector> [flags]

Flags:
  -h, --help   help for checked

Global Flags:
      --json             print the unified machine-readable output envelope (shorthand for --output json)
      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default "text")
      --session string   session name (default "default")
"###,
        ),
        "is enabled" => Some(
            r###"Inspect enabled

Usage:
  symbrowse is enabled <selector> [flags]

Flags:
  -h, --help   help for enabled

Global Flags:
      --json             print the unified machine-readable output envelope (shorthand for --output json)
      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default "text")
      --session string   session name (default "default")
"###,
        ),
        "is visible" => Some(
            r###"Inspect visible

Usage:
  symbrowse is visible <selector> [flags]

Flags:
  -h, --help   help for visible

Global Flags:
      --json             print the unified machine-readable output envelope (shorthand for --output json)
      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default "text")
      --session string   session name (default "default")
"###,
        ),
        "open" => Some(
            r###"Open a URL in the browser and wait for load

Usage:
  symbrowse open [url] [flags]

Flags:
  -h, --help             help for open
      --session string   session name (default "default")

Global Flags:
      --json            print the unified machine-readable output envelope (shorthand for --output json)
      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default "text")
"###,
        ),
        "press" => Some(
            r###"press performs the press interaction on the targeted element.

Accepted selector forms:
  - CSS selector (e.g. "button.submit", "#username", "input[name='q']")
  - Stable @eN ref from snapshot (e.g. "@e1", "@e2")
  - Role/name pair as supported by the engine (e.g. role and accessible name)

Optional [value] argument:
  The keyboard key to press (e.g. "Enter", "Tab", "Escape", "ArrowDown").

Usage:
  symbrowse press <selector> [value] [flags]

Flags:
  -h, --help             help for press
      --session string   session name (default "default")

Global Flags:
      --json            print the unified machine-readable output envelope (shorthand for --output json)
      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default "text")
"###,
        ),
        "read" => Some(
            r###"read renders the current page — or the page at url when given — and prints markdown with YAML frontmatter (title, url, fetched_at, lang, tokens_est, schema_type) in the symfetch output schema. --json prints the same document as the unified envelope. --engine-hint additionally reports whether JavaScript was actually needed for the content (js_required), so an agent can choose Tier 0 directly next time.

Usage:
  symbrowse read [url] [flags]

Flags:
      --content-boundaries   wrap page content in unforgeable boundary markers (default on in MCP mode)
      --engine-hint          report whether JavaScript was needed for the content (js_required with reason)
      --filter string        remove every subtree matching this CSS selector before rendering
  -h, --help                 help for read
      --max-tokens int       token budget for the payload; oversized output is truncated and stored in the cache (0 = no limit)
      --outline              return only the heading structure
      --raw                  return the page HTML instead of markdown
      --selector string      render only the first subtree matching this CSS selector
      --session string       session name (default "default")

Global Flags:
      --json            print the unified machine-readable output envelope (shorthand for --output json)
      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default "text")
"###,
        ),
        "snapshot" => Some(
            r###"Render the accessibility tree

Usage:
  symbrowse snapshot [flags]

Flags:
  -c, --compact                     omit non-interactive structural nodes
      --content-boundaries          wrap page content in unforgeable boundary markers (default on in MCP mode)
  -d, --depth int                   maximum tree depth; zero means unlimited
      --diff                        show changes since the previous snapshot
  -h, --help                        help for snapshot
      --injection-patterns string   custom prompt-injection pattern file (one phrase per line; replaces the embedded multilingual list)
  -i, --interactive                 include only interactive nodes
      --max-tokens int              token budget for the payload; oversized output is truncated and stored in the cache (0 = no limit)
      --no-injection-scan           disable the prompt-injection heuristic scan (hidden text, agent-directed imperatives, aria-label mismatch, alt/title/meta/comment instructions)
  -s, --selector string             select an accessibility subtree
      --session string              session name (default "default")
      --since string                show changes since a specific snapshot ID
  -u, --urls                        include link URLs

Global Flags:
      --json            print the unified machine-readable output envelope (shorthand for --output json)
      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default "text")
"###,
        ),
        "type" => Some(
            r###"type performs the type interaction on the targeted element.

Accepted selector forms:
  - CSS selector (e.g. "button.submit", "#username", "input[name='q']")
  - Stable @eN ref from snapshot (e.g. "@e1", "@e2")
  - Role/name pair as supported by the engine (e.g. role and accessible name)

Optional [value] argument:
  The text value to type into the element, appending to any existing content.

Usage:
  symbrowse type <selector> [value] [flags]

Flags:
  -h, --help             help for type
      --session string   session name (default "default")

Global Flags:
      --json            print the unified machine-readable output envelope (shorthand for --output json)
      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default "text")
"###,
        ),
        "wait" => Some(
            r###"Wait for a browser condition

Usage:
  symbrowse wait [selector] [flags]

Flags:
  -h, --help             help for wait
      --load string      wait for load, domcontentloaded, or networkidle
      --ms int           wait for milliseconds
      --session string   session name (default "default")
      --state string     selector state: visible, hidden, attached, or detached (default "visible")
      --text string      wait for text
      --url string       wait for a URL glob

Global Flags:
      --json            print the unified machine-readable output envelope (shorthand for --output json)
      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default "text")
"###,
        ),
        "back" => Some(
            r###"Navigate back in page history

Usage:
  symbrowse back [url] [flags]

Flags:
  -h, --help             help for back
      --session string   session name (default "default")

Global Flags:
      --json            print the unified machine-readable output envelope (shorthand for --output json)
      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default "text")
"###,
        ),
        "forward" => Some(
            r###"Navigate forward in page history

Usage:
  symbrowse forward [url] [flags]

Flags:
  -h, --help             help for forward
      --session string   session name (default "default")

Global Flags:
      --json            print the unified machine-readable output envelope (shorthand for --output json)
      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default "text")
"###,
        ),
        "reload" => Some(
            r###"Reload the current page

Usage:
  symbrowse reload [url] [flags]

Flags:
  -h, --help             help for reload
      --session string   session name (default "default")

Global Flags:
      --json            print the unified machine-readable output envelope (shorthand for --output json)
      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default "text")
"###,
        ),
        "daemon" => Some(
            r###"Run or inspect the symbrowse daemon

Usage:
  symbrowse daemon [flags]
  symbrowse daemon [command]

Available Commands:
  status      Show daemon status
  stop        Stop the running daemon

Flags:
      --allow-private            allow private and loopback targets when the SSRF guard is active
      --allowed-domains string   comma-separated domain allowlist (e.g. "example.com,*.example.com"); denies every other domain on the network layer
      --cdp-endpoint string      attach to an existing DevTools endpoint (e.g. http://127.0.0.1:9222) instead of launching Chrome; also via SYMBROWSE_CDP_ENDPOINT or config.toml
      --engine string            engine implementation: chrome (default), static (JS-free HTML reader), safari-attach (live Safari session via Apple Events), or safari-bidi (isolated Safari via safaridriver --bidi) (default "chrome")
      --headless                 launch Chrome in headless mode (no GUI session; also via SYMBROWSE_HEADLESS=1)
  -h, --help                     help for daemon
      --profile string           reuse an existing Chrome profile (name or path) instead of a private session profile
      --restore string           restore the named state when the session browser starts
      --session string           daemon session name (default "default")
      --ssrf                     enable the SSRF guard: RFC1918, loopback, link-local, .local, and IPv6-ULA targets are denied (default on in MCP mode)

Global Flags:
      --json            print the unified machine-readable output envelope (shorthand for --output json)
      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default "text")

Use "symbrowse daemon [command] --help" for more information about a command.
"###,
        ),
        "daemon status" => Some(
            r###"Show daemon status

Usage:
  symbrowse daemon status [flags]

Flags:
  -h, --help   help for status

Global Flags:
      --json             print the unified machine-readable output envelope (shorthand for --output json)
      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default "text")
      --profile string   reuse an existing Chrome profile (name or path) instead of a private session profile
      --restore string   restore the named state when the session browser starts
      --session string   daemon session name (default "default")
"###,
        ),
        "daemon stop" => Some(
            r###"Stop the running daemon

Usage:
  symbrowse daemon stop [flags]

Flags:
  -h, --help   help for stop

Global Flags:
      --json             print the unified machine-readable output envelope (shorthand for --output json)
      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default "text")
      --profile string   reuse an existing Chrome profile (name or path) instead of a private session profile
      --restore string   restore the named state when the session browser starts
      --session string   daemon session name (default "default")
"###,
        ),
        _ => None,
    }
}
