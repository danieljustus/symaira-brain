// Scenario declarations and wire records for the Go request oracle.
package main

// scriptStep is one canned transport response: either a status/body pair or a
// transport error. Every case declares exactly the steps the shipped strategy
// may consume; exhausting the script is itself recorded as an error.
type scriptStep struct {
	Status int    `json:"status,omitempty"`
	Body   string `json:"body,omitempty"`
	Error  string `json:"error,omitempty"`
}

// headerPair keeps the dump explicit, like scripts/usage-request-oracle.
type headerPair struct {
	Name  string `json:"name"`
	Value string `json:"value"`
}

type capturedRequest struct {
	Method  string       `json:"method"`
	URL     string       `json:"url"`
	Headers []headerPair `json:"headers"`
	Body    string       `json:"body,omitempty"`
}

type meterSummary struct {
	Label        string  `json:"label"`
	Used         *string `json:"used"`
	Limit        *string `json:"limit"`
	Unit         string  `json:"unit"`
	ResetsAt     bool    `json:"resets_at"`
	ResetAfterNS *int64  `json:"reset_after_ns"`
}

type snapshotSummary struct {
	Source string         `json:"source"`
	Meters []meterSummary `json:"meters"`
}

type result struct {
	Kind     string           `json:"kind"` // ok | error
	Class    string           `json:"class,omitempty"`
	Text     string           `json:"text,omitempty"`
	Snapshot *snapshotSummary `json:"snapshot,omitempty"`
}

type recordedCase struct {
	ID        string            `json:"id"`
	Env       map[string]string `json:"env"`
	Script    []scriptStep      `json:"script"`
	Workspace *string           `json:"workspace"`
	Executed  []capturedRequest `json:"executed"`
	Result    result            `json:"result"`
}

type provenance struct {
	OracleRevision string            `json:"oracle_revision"`
	Toolchain      string            `json:"toolchain"`
	SourceHashes   map[string]string `json:"source_hashes"`
	HarnessHashes  map[string]string `json:"harness_hashes"`
	Generator      string            `json:"generator"`
	Command        string            `json:"command"`
	// CheckCommand is the byte-for-byte recheck the slice must run.
	CheckCommand string `json:"check_command"`
	OracleSource string `json:"oracle_source"`
	// OracleSources lists every shipped Go file an anchor verbatim occurs in
	// (the subscription parse text lives in opencode_parse.go).
	OracleSources []string `json:"oracle_sources"`
	OracleAnchors []string `json:"oracle_anchors"`
	Testdata      []string `json:"testdata"`
	Transport     string   `json:"transport"`
}

type dump struct {
	SchemaVersion int            `json:"schema_version"`
	Provenance    provenance     `json:"provenance"`
	Cases         []recordedCase `json:"cases"`
}

// testCase declares one scenario: which environment the real provider sees
// and which bounded responses the recording transport serves, in order.
type testCase struct {
	id        string
	cookie    bool    // OPENCODE_COOKIE set
	workspace *string // OPENCODE_WORKSPACE_ID (nil = unset)
	script    []scriptStep
}

func ws(value string) *string { return &value }

// Response bodies. The two JSON bodies are the shipped Go testdata fixtures,
// loaded in main before the case table is built, so the corpus stays bound to
// the parser inputs the Go tests use.
var (
	bodyWorkspaces   string
	bodySubscription string
)

const (
	bodyNoWorkspace  = `{"workspaces":[]}`
	bodyEmpty        = `{}`
	bodySignedOut    = `{"error":"please sign in"}`
	bodyMalformed    = "not json at all"
	bodyMalformedTwo = "still not json"
	bodyServerError  = "oops"
	bodySignedOut500 = "Please sign in to continue"
)

// probeScript serves a body with no workspace id and no usage fields, so the
// workspace leg (no override id) and the subscription leg (override id) each
// run exactly one request per attempt.
func probeScript() []scriptStep {
	return []scriptStep{{Status: 200, Body: bodyEmpty}, {Status: 200, Body: bodyEmpty}}
}

// cases is the declared corpus. main asserts the declared and executed case
// IDs are equal; the Rust port repeats that assertion against the fixture.
// Built after the testdata bodies are loaded.
func cases() []testCase {
	return []testCase{
		// Workspace discovery: GET, then POST fallback, then signed-out and
		// status handling (internal/usage/opencode.go fetchWorkspaceID).
		{
			id:     "workspace_then_subscription_ok",
			cookie: true,
			script: []scriptStep{{Status: 200, Body: bodyWorkspaces}, {Status: 200, Body: bodySubscription}},
		},
		{
			id:     "workspace_post_fallback_ok",
			cookie: true,
			script: []scriptStep{
				{Status: 200, Body: bodyNoWorkspace},
				{Status: 200, Body: bodyWorkspaces},
				{Status: 200, Body: bodySubscription},
			},
		},
		{
			id:     "workspace_lookup_missing_id",
			cookie: true,
			script: []scriptStep{{Status: 200, Body: bodyEmpty}, {Status: 200, Body: bodyEmpty}},
		},
		{
			id:     "workspace_signed_out_get",
			cookie: true,
			script: []scriptStep{{Status: 200, Body: bodySignedOut}},
		},
		{
			id:     "workspace_signed_out_post",
			cookie: true,
			script: []scriptStep{{Status: 200, Body: bodyNoWorkspace}, {Status: 200, Body: bodySignedOut}},
		},
		{
			id:     "workspace_http_401",
			cookie: true,
			script: []scriptStep{{Status: 401, Body: `{"error":"nope"}`}},
		},
		{
			id:     "workspace_http_500_signed_out_body",
			cookie: true,
			script: []scriptStep{{Status: 500, Body: bodySignedOut500}},
		},
		{
			id:     "workspace_http_500",
			cookie: true,
			script: []scriptStep{{Status: 500, Body: bodyServerError}},
		},

		// Subscription fetch with an explicit override: GET, parse-check,
		// POST fallback, signed-out (fetchSubscriptionInfo).
		{
			id:        "subscription_override_ok",
			cookie:    true,
			workspace: ws("wrk_override1"),
			script:    []scriptStep{{Status: 200, Body: bodySubscription}},
		},
		{
			id:        "subscription_url_override_ok",
			cookie:    true,
			workspace: ws("https://opencode.ai/workspace/wrk_urlform9/billing"),
			script:    []scriptStep{{Status: 200, Body: bodySubscription}},
		},
		{
			id:        "subscription_post_fallback_ok",
			cookie:    true,
			workspace: ws("wrk_override1"),
			script: []scriptStep{
				{Status: 200, Body: bodyMalformed},
				{Status: 200, Body: bodySubscription},
			},
		},
		{
			id:        "subscription_unparseable_both",
			cookie:    true,
			workspace: ws("wrk_override1"),
			script: []scriptStep{
				{Status: 200, Body: bodyMalformed},
				{Status: 200, Body: bodyMalformedTwo},
			},
		},
		{
			id:        "subscription_signed_out_get",
			cookie:    true,
			workspace: ws("wrk_override1"),
			script:    []scriptStep{{Status: 200, Body: bodySignedOut}},
		},
		{
			id:        "subscription_signed_out_post",
			cookie:    true,
			workspace: ws("wrk_override1"),
			script: []scriptStep{
				{Status: 200, Body: bodyMalformed},
				{Status: 200, Body: bodySignedOut},
			},
		},

		// Transport failure and the workspace-only configuration gate
		// (Strategies() is empty without a cookie).
		{
			id:        "network_error_first_request",
			cookie:    true,
			workspace: ws("wrk_override1"),
			script:    []scriptStep{{Error: "boom"}},
		},
		{
			id:        "workspace_only_no_cookie",
			cookie:    false,
			workspace: ws("dump-workspace"),
			script:    nil,
		},

		// Workspace-id normalization probes (openCodeNormalizeWorkspaceID):
		// the observed normalized id is derived from the first executed
		// request.
		{id: "normalize_bare_id", cookie: true, workspace: ws("wrk_abc123"), script: probeScript()},
		{id: "normalize_url_full", cookie: true, workspace: ws("https://opencode.ai/workspace/wrk_abc123/billing"), script: probeScript()},
		{id: "normalize_url_padded", cookie: true, workspace: ws("  https://opencode.ai/workspace/wrk_xyz987  "), script: probeScript()},
		{id: "normalize_embedded_text", cookie: true, workspace: ws("text wrk_embedded42 more"), script: probeScript()},
		{id: "normalize_empty", cookie: true, workspace: ws(""), script: probeScript()},
		{id: "normalize_url_no_id", cookie: true, workspace: ws("https://opencode.ai/workspace/"), script: probeScript()},
		{id: "normalize_wrk_underscore_only", cookie: true, workspace: ws("wrk_"), script: probeScript()},
		{id: "normalize_token_prefix", cookie: true, workspace: ws("prefix_wrk_trail9"), script: probeScript()},
		{id: "normalize_url_query", cookie: true, workspace: ws("https://opencode.ai/workspace/wrk_query7/billing?plan=pro"), script: probeScript()},
		{id: "normalize_url_fragment", cookie: true, workspace: ws("https://opencode.ai/workspace/wrk_frag8/billing#top"), script: probeScript()},
		{id: "normalize_bad_percent", cookie: true, workspace: ws("https://opencode.ai/workspace/wrk_bad%zz"), script: probeScript()},
		{id: "normalize_dump_workspace", cookie: true, workspace: ws("dump-workspace"), script: probeScript()},
		{id: "normalize_wrk_space", cookie: true, workspace: ws("wrk_a b"), script: probeScript()},
		{id: "normalize_wrk_quote", cookie: true, workspace: ws(`wrk_a"b`), script: probeScript()},
		{id: "normalize_wrk_html", cookie: true, workspace: ws("wrk_a<b"), script: probeScript()},
		{id: "normalize_wrk_star", cookie: true, workspace: ws("wrk_a*b"), script: probeScript()},
		{id: "normalize_wrk_line_sep", cookie: true, workspace: ws("wrk_a\u2028b"), script: probeScript()},
	}
}
