package main

// Review regressions drive the production Go provider; these are peer inputs,
// never hand-written expected snapshots or request sequences.
func reviewCases() []testCase {
	var result []testCase
	for _, item := range []struct{ id, body string }{
		{"js_get", `{rollingUsage:{usagePercent:12.34,resetInSec:3600},weeklyUsage:{usagePercent:45.6,resetInSec:86400}}`},
		{"json_custom", `{"custom":{"usagePercent":12}}`},
		{"json_numeric_string", `{"usagePercent":"12"}`},
		{"json_named_precedence", `{"usagePercent":99,"rollingUsage":{"usagePercent":12}}`},
		{"json_fractional_reset", `{"usagePercent":12,"resetInSec":1.25}`},
		{"json_fractional_negative_reset", `{"usagePercent":12,"resetInSec":-0.25}`},
		{"js_generic", `usagePercent: 12.5, resetIn: 10`},
		{"js_weekly_without_reset", `rollingUsage:{usagePercent:12,resetInSec:10},weeklyUsage:{usagePercent:20}`},
		{"js_missing_reset", `rollingUsage:{usagePercent:12}`},
		{"js_ascii_space", "rollingUsage:{usagePercent:\v12,resetInSec:10}"},
		{"signed_out_unicode", "SİGN IN"},
	} {
		result = append(result, testCase{id: item.id, cookie: true, workspace: ws("wrk_review"), script: []scriptStep{{Status: 200, Body: item.body}, {Status: 200, Body: bodyEmpty}}})
	}
	result = append(result, testCase{id: "js_post", cookie: true, workspace: ws("wrk_review"), script: []scriptStep{{Status: 200, Body: bodyEmpty}, {Status: 200, Body: `{rollingUsage:{usagePercent:12,resetInSec:3600}}`}}})
	for _, item := range []struct{ id, value string }{
		{"url_encoded_id", "https://opencode.ai/workspace/%77rk_abc"},
		{"url_encoded_letter", "https://opencode.ai/workspace/wrk_%61bc"},
		{"url_encoded_slash", "https://opencode.ai/workspace/wrk_a%2Fb"},
		{"url_encoded_space", "https://opencode.ai/workspace/wrk_a%20b"},
		{"url_invalid_host_escape", "https://bad%zz/workspace/wrk_a-b"},
		{"url_invalid_user_escape", "https://bad%zz@opencode.ai/workspace/wrk_a-b"},
		{"url_large_port", "https://opencode.ai:65536/workspace/wrk_a-b"},
		{"url_plus_port", "https://opencode.ai:+80/workspace/wrk_a-b"},
		{"url_empty_port", "https://opencode.ai:/workspace/wrk_a-b"},
		{"url_invalid_host_space", "https://bad host/workspace/wrk_a-b"},
		{"url_invalid_user_space", "https://bad user@opencode.ai/workspace/wrk_a-b"},
		{"url_relative_three_slashes", "///workspace/wrk_a-b"},
		{"url_opaque", "custom:workspace/wrk_a-b"},
		{"url_empty_scheme", ":workspace/wrk_a-b"},
	} {
		result = append(result, testCase{id: item.id, cookie: true, workspace: ws(item.value), script: probeScript()})
	}
	return result
}
