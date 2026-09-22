package main

import (
	"bytes"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/guard/cmd/symguard/decide"
	"github.com/danieljustus/symaira-brain/guard/internal/audit"
	"github.com/danieljustus/symaira-brain/guard/internal/model"
)

// SEC-002 cases: guard decision output bytes pinned from the real
// `symguard decide` command (guard/cmd/symguard/decide).
//
// decide.Run has no clock seam. Response bytes are clock-independent: the
// only clock use is the deadline comparison, and the corpus pins deadlines
// far in the past/future so wall-clock drift cannot change the outcome
// (the same exclusion the external guard-decide oracle documents for its
// deadline-equality probe). The audit record's two wall-clock fields (id,
// decided_at) are re-derived at the frozen instant below with the same
// production primitives decide.record uses; every other field comes from
// the record the real command produced.

// frozenDecideNow is the injected instant for decide_audit cases.
const frozenDecideNow = "2026-08-06T12:00:00Z"

type decideRequestInput struct {
	Request   string `json:"request"`
	SinkError string `json:"sink_error,omitempty"`
}

type decideAuditInput struct {
	Request string `json:"request"`
	Now     string `json:"now"`
}

// captureSink records the audit record the real command produced; err (when
// set) makes the sink fail so the fail-closed flip can be pinned.
type captureSink struct {
	record audit.ExternalDecision
	err    error
}

func (s *captureSink) Write(rec audit.ExternalDecision) error {
	s.record = rec
	return s.err
}

func sec002Cases() []oracleCase {
	var cases []oracleCase

	for _, tc := range []struct {
		id, request, sinkError string
	}{
		{"decide_low_allow", `{"command":"browse","risk_class":"low"}`, ""},
		{"decide_medium_allow", `{"command":"browse","risk_class":"medium"}`, ""},
		{"decide_low_warnings_confirm", `{"command":"browse","risk_class":"low","warnings":["slow network"]}`, ""},
		{"decide_high_confirm", `{"command":"browse","risk_class":"high"}`, ""},
		{"decide_high_warnings_deny", `{"command":"browse","risk_class":"high","warnings":["cert expired","redirected"]}`, ""},
		{"decide_critical_loopback_allow", `{"command":"probe","risk_class":"critical","domain":"127.0.0.1"}`, ""},
		{"decide_critical_trimmed_allow", `{"command":"probe","risk_class":" critical ","domain":" 127.0.0.1 "}`, ""},
		{"decide_critical_ipv6_allow", `{"command":"probe","risk_class":"critical","domain":"::1"}`, ""},
		{"decide_critical_hostname_deny", `{"command":"probe","risk_class":"critical","domain":"example.com"}`, ""},
		{"decide_critical_warning_deny", `{"command":"probe","risk_class":"critical","domain":"127.0.0.1","warnings":["check"]}`, ""},
		{"decide_unknown_risk_deny", `{"command":"browse","risk_class":"urgent"}`, ""},
		{"decide_missing_risk_deny", `{"command":"browse"}`, ""},
		{"decide_missing_command_deny", `{"risk_class":"low"}`, ""},
		{"decide_empty_deny", ``, ""},
		{"decide_whitespace_deny", "  \n\t ", ""},
		{"decide_truncated_deny", `{"command":"browse"`, ""},
		{"decide_trailing_deny", `{"command":"browse"}x`, ""},
		{"decide_warnings_type_error_deny", `{"command":"browse","warnings":"not-a-list"}`, ""},
		{"decide_deadline_expired_deny", `{"command":"browse","risk_class":"low","deadline":"2000-01-01T00:00:00Z"}`, ""},
		{"decide_deadline_future_allow", `{"command":"browse","risk_class":"low","deadline":"2999-01-01T00:00:00Z"}`, ""},
		{"decide_oversized_deny", strings.Repeat("x", 65537), ""},
		{"decide_audit_failure_flips_allow", `{"command":"browse","risk_class":"low"}`, "audit sink unavailable"},
		{"decide_audit_failure_keeps_missing_command_deny", `{"risk_class":"low"}`, "audit sink unavailable"},
		{"decide_audit_failure_keeps_unknown_risk_deny", `{"command":"b","risk_class":"zz"}`, "disk full"},
	} {
		cases = append(cases, decideResponseCase(tc.id, tc.request, tc.sinkError))
	}

	for _, tc := range []struct{ id, request string }{
		{"decide_audit_allow", `{"command":"browse","risk_class":"low"}`},
		{"decide_audit_confirm_warnings", `{"command":"browse","risk_class":"high","warnings":["one"," two "]}`},
		{"decide_audit_deny_unknown_risk", `{"command":"browse","risk_class":"nope","domain":"host"}`},
		{"decide_audit_deny_deadline_expired", `{"command":"browse","risk_class":"low","deadline":"2000-01-01T00:00:00Z"}`},
		{"decide_audit_deny_empty_request", ``},
		{"decide_audit_deny_type_error", `{"command":"browse","risk_class":42}`},
	} {
		cases = append(cases, decideAuditCase(tc.id, tc.request))
	}

	return cases
}

// decideResponseCase pins the exact stdout bytes of the real command for
// one request, including the audit-sink failure flip.
func decideResponseCase(id, request, sinkError string) oracleCase {
	sink := &captureSink{}
	if sinkError != "" {
		sink.err = errors.New(sinkError)
	}
	var out bytes.Buffer
	if code := decide.Run(nil, strings.NewReader(request), &out, sink); code != 0 {
		panic(fmt.Sprintf("decide case %s: unexpected exit code %d", id, code))
	}
	return successCase(id, "decide_response", decideRequestInput{Request: request, SinkError: sinkError}, out.String())
}

// decideAuditCase pins the exact audit record bytes for one request with
// the wall-clock fields frozen (see the file comment).
func decideAuditCase(id, request string) oracleCase {
	sink := &captureSink{}
	var out bytes.Buffer
	if code := decide.Run(nil, strings.NewReader(request), &out, sink); code != 0 {
		panic(fmt.Sprintf("decide case %s: unexpected exit code %d", id, code))
	}
	frozen := parseTime(frozenDecideNow).UTC()
	record := sink.record
	record.ID = model.EventID(model.SourceDecide, frozen.UnixNano())
	record.DecidedAt = frozen.Format(time.RFC3339)
	return successCase(id, "decide_audit", decideAuditInput{Request: request, Now: frozenDecideNow}, record)
}
