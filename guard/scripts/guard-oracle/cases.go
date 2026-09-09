package main

import (
	"fmt"
	"time"

	"github.com/danieljustus/symaira-brain/guard/internal/model"
	"github.com/danieljustus/symaira-brain/guard/internal/policy"
)

type policyInput struct {
	Rules   []policy.Rule  `json:"rules"`
	Call    model.ToolCall `json:"call"`
	Default model.Decision `json:"default"`
	Trace   bool           `json:"trace"`
}

type mergeInput struct {
	Left  policy.Catalog `json:"left"`
	Right policy.Catalog `json:"right"`
}

type scopeInput struct {
	Scope      []string      `json:"scope"`
	Capability string        `json:"capability"`
	Result     policy.Result `json:"result"`
}

type marginalInput struct {
	Tool           string          `json:"tool"`
	AlreadyAllowed map[string]bool `json:"already_allowed"`
}

type marginalOutput struct {
	Marginal   bool             `json:"marginal"`
	Reason     string           `json:"reason,omitempty"`
	Risk       policy.RiskLevel `json:"risk"`
	RiskReason string           `json:"risk_reason,omitempty"`
}

type expiryInput struct {
	Deadline string `json:"deadline"`
	Now      string `json:"now"`
}

type noDecisionInput struct {
	Failure    model.FailureMode `json:"failure_mode"`
	Diagnostic string            `json:"diagnostic"`
}

func makeRule(id string, precedence policy.Precedence, decision model.Decision, match policy.MatchCriteria, reason string) policy.Rule {
	return policy.Rule{
		ID: policy.RuleID(id), Version: "1.0.0", Precedence: precedence,
		Decision: decision, Match: match, Reason: reason,
	}
}

func buildSuite() oracleSuite {
	var cases []oracleCase
	for _, value := range []model.SourceType{model.SourceProxy, model.SourceHook, model.SourceArtifact, model.SourceScan, model.SourceDecide} {
		cases = append(cases, successCase("source_"+string(value), "validate_source", string(value), true))
	}
	cases = append(cases,
		errorCase("source_unknown", "validate_source", "future", model.ValidateSource("future")),
		successCase("decision_allow", "validate_decision", "allow", true),
		errorCase("decision_unknown", "validate_decision", "future", model.ValidateDecision("future")),
	)

	for _, tc := range []struct {
		id      string
		failure model.FailureMode
	}{
		{"no_decision_default", ""},
		{"no_decision_deny", model.FailureModeDeny},
		{"no_decision_allow", model.FailureModeAllow},
		{"no_decision_unknown", model.FailureMode("bogus")},
	} {
		nd := model.NewNoDecision(tc.failure, "fixed diagnostic")
		cases = append(cases, successCase(tc.id, "no_decision", noDecisionInput{Failure: tc.failure, Diagnostic: "fixed diagnostic"}, nd.Control))
	}

	cases = append(cases,
		successCase("event_id_injected", "event_id", map[string]any{"source": "proxy", "counter": int64(42)}, model.EventID(model.SourceProxy, 42)),
	)
	for _, tc := range []struct {
		id, deadline, now string
	}{
		{"expiry_zero", "", "2026-08-06T12:00:00Z"},
		{"expiry_future", "2026-08-06T13:00:00Z", "2026-08-06T12:00:00Z"},
		{"expiry_exact", "2026-08-06T12:00:00Z", "2026-08-06T12:00:00Z"},
		{"expiry_past", "2026-08-06T11:00:00Z", "2026-08-06T12:00:00Z"},
	} {
		request := model.DecisionRequest{Deadline: parseTime(tc.deadline)}
		cases = append(cases, successCase(tc.id, "expired", expiryInput{tc.deadline, tc.now}, request.Expired(parseTime(tc.now))))
	}
	validRequest := model.DecisionRequest{Call: model.ToolCall{Server: "filesystem", Tool: "read_file"}}
	cases = append(cases, successCase("request_valid", "validate_request", validRequest, true))
	invalidRequest := model.DecisionRequest{Call: model.ToolCall{Tool: "read_file"}}
	cases = append(cases, errorCase("request_missing_server", "validate_request", invalidRequest, model.ValidateDecisionRequest(invalidRequest)))
	unknownFailureRequest := model.DecisionRequest{Call: validRequest.Call, Failure: model.FailureMode("bogus")}
	cases = append(cases, errorCase("request_unknown_failure", "validate_request", unknownFailureRequest, model.ValidateDecisionRequest(unknownFailureRequest)))

	validEvent := validActionEvent()
	cases = append(cases, successCase("event_valid", "validate_event", validEvent, true))
	for _, tc := range []struct {
		id     string
		mutate func(*model.ActionEvent)
	}{
		{"event_missing_id", func(e *model.ActionEvent) { e.ID = "" }},
		{"event_bad_schema", func(e *model.ActionEvent) { e.SchemaVer = model.SchemaVersion + 1 }},
		{"event_unknown_source", func(e *model.ActionEvent) { e.Source = model.SourceType("future") }},
		{"event_unknown_state", func(e *model.ActionEvent) { e.State = model.ActionState("future") }},
		{"event_missing_timestamp", func(e *model.ActionEvent) { e.Timestamp = "" }},
		{"event_missing_agent", func(e *model.ActionEvent) { e.Agent.AgentID = "" }},
		{"event_missing_server", func(e *model.ActionEvent) { e.Call.Server = "" }},
		{"event_missing_tool", func(e *model.ActionEvent) { e.Call.Tool = "" }},
		{"event_bad_control_decision", func(e *model.ActionEvent) {
			e.ControlResp = &model.ControlResponse{Decision: model.Decision("future")}
		}},
		{"event_bad_retry_after", func(e *model.ActionEvent) {
			e.ControlResp = &model.ControlResponse{Decision: model.DecisionAllow, RetryAfter: -1}
		}},
		{"event_bad_evaluation", func(e *model.ActionEvent) {
			e.Evaluation = &model.Evaluation{Decision: model.Decision("future")}
		}},
	} {
		event := validEvent
		tc.mutate(&event)
		cases = append(cases, errorCase(tc.id, "validate_event", event, model.ValidateActionEvent(event)))
	}

	fsRead := policy.MatchCriteria{Server: "filesystem", Tool: "read_file"}
	policyCases := []struct {
		id    string
		input policyInput
	}{
		{"policy_deny_bucket_wins", policyInput{
			Rules: []policy.Rule{
				makeRule("allow-1", 10, model.DecisionAllow, fsRead, "allow first"),
				makeRule("deny-1", 20, model.DecisionDeny, fsRead, "deny wins"),
			}, Call: model.ToolCall{Server: "filesystem", Tool: "read_file"}, Default: model.DecisionAllow,
		}},
		{"policy_require_failure", policyInput{
			Rules: []policy.Rule{
				makeRule("req-1", 10, model.DecisionRequire, policy.MatchCriteria{Tool: "read_file"}, "must read"),
				makeRule("allow-1", 20, model.DecisionAllow, policy.MatchCriteria{Server: "filesystem"}, "filesystem"),
			}, Call: model.ToolCall{Server: "filesystem", Tool: "write_file"}, Default: model.DecisionAllow,
		}},
		{"policy_defensive_deny", policyInput{
			Rules: []policy.Rule{makeRule("deny-cmd", 10, model.DecisionDeny, policy.MatchCriteria{CommandContains: []string{"rm -rf"}}, "danger")},
			Call:  model.ToolCall{Server: "filesystem", Tool: "execute", Args: float64(42)}, Default: model.DecisionAllow,
		}},
		{"policy_trace", policyInput{
			Rules: []policy.Rule{
				makeRule("deny-1", 10, model.DecisionDeny, policy.MatchCriteria{Server: "db"}, "db denied"),
				makeRule("req-1", 20, model.DecisionRequire, policy.MatchCriteria{Tool: "read_file"}, "read required"),
				makeRule("allow-1", 30, model.DecisionAllow, fsRead, "read allowed"),
			}, Call: model.ToolCall{Server: "filesystem", Tool: "read_file"}, Default: model.DecisionDeny, Trace: true,
		}},
		{"policy_remote_match", policyInput{
			Rules: []policy.Rule{makeRule("remote-1", 10, model.DecisionAsk, policy.MatchCriteria{Remote: "remote-a"}, "remote ask")},
			Call:  model.ToolCall{Server: "s", Tool: "t", Remote: "remote-a"}, Default: model.DecisionDeny,
		}},
		{"policy_remote_missing", policyInput{
			Rules: []policy.Rule{makeRule("remote-1", 10, model.DecisionAsk, policy.MatchCriteria{Remote: "remote-a"}, "remote ask")},
			Call:  model.ToolCall{Server: "s", Tool: "t"}, Default: model.DecisionDeny,
		}},
		{"policy_remote_mismatch", policyInput{
			Rules: []policy.Rule{makeRule("remote-1", 10, model.DecisionAsk, policy.MatchCriteria{Remote: "remote-a"}, "remote ask")},
			Call:  model.ToolCall{Server: "s", Tool: "t", Remote: "remote-b"}, Default: model.DecisionDeny,
		}},
	}
	for _, tc := range policyCases {
		catalog, err := policy.NewCatalog(tc.input.Rules, "1.0.0")
		if err != nil {
			panic(err)
		}
		var result policy.Result
		if tc.input.Trace {
			result = catalog.EvaluateOpts(tc.input.Call, tc.input.Default, policy.Options{Trace: true})
		} else {
			result = catalog.Evaluate(tc.input.Call, tc.input.Default)
		}
		cases = append(cases, successCase(tc.id, "evaluate", tc.input, result))
	}
	invalidCatalog := policy.Catalog{Rules: []policy.Rule{
		makeRule("good", 20, model.DecisionAllow, policy.MatchCriteria{Server: "s"}, "good"),
		makeRule("bad-match", 10, model.DecisionAllow, policy.MatchCriteria{}, "bad"),
	}, Version: "1.0.0"}
	cases = append(cases, errorCase("catalog_criteria_before_precedence", "catalog_validate", invalidCatalog, invalidCatalog.Validate()))
	invalidDecisionCatalog := policy.Catalog{Rules: []policy.Rule{
		makeRule("bad-decision", 10, model.Decision("future"), policy.MatchCriteria{Server: "s"}, "bad"),
	}, Version: "1.0.0"}
	cases = append(cases, errorCase("catalog_unknown_decision", "catalog_validate", invalidDecisionCatalog, invalidDecisionCatalog.Validate()))

	left, _ := policy.NewCatalog([]policy.Rule{makeRule("left", 10, model.DecisionAllow, policy.MatchCriteria{Server: "s"}, "left")}, "1.0.0")
	right, _ := policy.NewCatalog([]policy.Rule{makeRule("right", 10, model.DecisionDeny, policy.MatchCriteria{Server: "s"}, "right")}, "1.0.0")
	merged, _ := left.Merge(right)
	cases = append(cases, successCase("merge_precedence", "merge", mergeInput{*left, *right}, merged))
	duplicate, _ := policy.NewCatalog([]policy.Rule{makeRule("left", 10, model.DecisionDeny, policy.MatchCriteria{Server: "x"}, "duplicate")}, "1.0.0")
	duplicateMerged, duplicateErr := left.Merge(duplicate)
	_ = duplicateMerged
	cases = append(cases, errorCase("merge_duplicate", "merge", mergeInput{*left, *duplicate}, duplicateErr))

	for _, tc := range []struct {
		id string
		in scopeInput
	}{
		{"scope_pass", scopeInput{[]string{"shell"}, "shell", policy.Result{Decision: model.DecisionAllow, Reason: "allowed", Matched: true}}},
		{"scope_narrow", scopeInput{[]string{"read_public"}, "shell", policy.Result{Decision: model.DecisionAllow, Reason: "allowed", Matched: true}}},
		{"scope_denied_passthrough", scopeInput{[]string{"*"}, "shell", policy.Result{Decision: model.DecisionDeny, Reason: "blocked", Matched: true}}},
	} {
		cases = append(cases, successCase(tc.id, "scope_ceiling", tc.in, policy.ScopeCeiling(tc.in.Scope, tc.in.Capability, tc.in.Result)))
	}

	for _, tc := range []struct {
		id string
		in marginalInput
	}{
		{"marginal_shell_private", marginalInput{"read_private", map[string]bool{"shell": true}}},
		{"marginal_false_value_ignored", marginalInput{"read_private", map[string]bool{"shell": false}}},
		{"marginal_unknown", marginalInput{"unknown", map[string]bool{"shell": true}}},
		{"marginal_credential_use_does_not_cover_secret", marginalInput{"read_secret", map[string]bool{"credential_use": true}}},
		{"marginal_secret_does_not_cover_credential_use", marginalInput{"credential_use", map[string]bool{"read_secret": true}}},
	} {
		marginal := policy.MarginalCapabilityCheck(tc.in.Tool, tc.in.AlreadyAllowed)
		risk, riskReason := policy.ClassifyRiskWithReason(tc.in.Tool, marginal, tc.in.AlreadyAllowed)
		cases = append(cases, successCase(tc.id, "marginal", tc.in, marginalOutput{Marginal: marginal, Reason: policy.MarginalCapabilityReason(tc.in.Tool, tc.in.AlreadyAllowed), Risk: risk, RiskReason: riskReason}))
	}
	return oracleSuite{Cases: cases}
}

func validActionEvent() model.ActionEvent {
	return model.ActionEvent{
		ID:        model.EventID(model.SourceProxy, 1),
		SchemaVer: model.SchemaVersion,
		Source:    model.SourceProxy,
		Agent:     model.AgentIdentity{AgentID: "agent-1"},
		Call:      model.ToolCall{Server: "filesystem", Tool: "read_file"},
		State:     model.ActionRequested,
		Timestamp: "2026-08-06T12:00:00Z",
	}
}

func parseTime(value string) time.Time {
	if value == "" {
		return time.Time{}
	}
	parsed, err := time.Parse(time.RFC3339, value)
	if err != nil {
		panic(fmt.Sprintf("parse fixture time %q: %v", value, err))
	}
	return parsed
}
