package decide

import (
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-guard/internal/audit"
)

func TestEvaluate(t *testing.T) {
	tests := []struct {
		name      string
		req       request
		want      outcome
		wantRegex string // substring match on the reason
	}{
		{"low no warnings allows", request{Command: "open https://x", RiskClass: "low"}, allow, "low risk class, no warnings"},
		{"low with warnings confirms", request{Command: "open https://x", RiskClass: "low", Warnings: []string{"popup"}}, confirm, "low risk class with warnings: popup"},
		{"medium no warnings allows", request{Command: "fetch", RiskClass: "medium"}, allow, "medium risk class, no warnings"},
		{"medium with warnings confirms", request{Command: "fetch", RiskClass: "medium", Warnings: []string{"redirect"}}, confirm, "medium risk class with warnings: redirect"},
		{"high no warnings confirms", request{Command: "shell", RiskClass: "high"}, confirm, "high risk class requires confirmation"},
		{"high with warnings denies", request{Command: "shell", RiskClass: "high", Warnings: []string{"sudo"}}, deny, "high risk class with warnings: sudo"},
		{"critical loopback no warnings allows", request{Command: "curl localhost", RiskClass: "critical", Domain: "localhost"}, allow, `allowlisted domain "localhost"`},
		{"critical ipv4 loopback allows", request{Command: "curl", RiskClass: "critical", Domain: "127.0.0.1"}, allow, "allowlisted domain"},
		{"critical remote domain denies", request{Command: "curl", RiskClass: "critical", Domain: "github.com"}, deny, "critical risk class: requires allowlisted domain"},
		{"critical with warnings denies even loopback", request{Command: "curl", RiskClass: "critical", Domain: "localhost", Warnings: []string{"tls"}}, deny, "critical risk class: requires allowlisted domain"},
		{"critical empty domain denies", request{Command: "curl", RiskClass: "critical"}, deny, "critical risk class: requires allowlisted domain"},
		{"unknown risk class denies", request{Command: "x", RiskClass: "bogus"}, deny, `unknown risk class "bogus"`},
		{"empty risk class denies", request{Command: "x"}, deny, `unknown risk class ""`},
		{"risk class is case-insensitive", request{Command: "shell", RiskClass: "HIGH"}, confirm, "high risk class requires confirmation"},
		{"domain is case-insensitive", request{Command: "curl", RiskClass: "critical", Domain: "LOCALHOST"}, allow, "allowlisted domain"},
		{"whitespace-only warnings dropped", request{Command: "open", RiskClass: "low", Warnings: []string{"", "  "}}, allow, "low risk class, no warnings"},
		{"mixed warnings keep order", request{Command: "fetch", RiskClass: "medium", Warnings: []string{" a ", "", "b"}}, confirm, "medium risk class with warnings: a; b"},
		{"trailing space in risk class tolerated", request{Command: "x", RiskClass: "high "}, confirm, "high risk class requires confirmation"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, reason := evaluate(tt.req)
			if got != tt.want {
				t.Errorf("evaluate() decision = %q, want %q", got, tt.want)
			}
			if !strings.Contains(reason, tt.wantRegex) {
				t.Errorf("evaluate() reason = %q, want substring %q", reason, tt.wantRegex)
			}
		})
	}
}

func TestDecideRequest(t *testing.T) {
	now := time.Date(2026, 8, 6, 12, 0, 0, 0, time.UTC)
	tests := []struct {
		name      string
		input     string
		want      outcome
		wantRegex string
	}{
		{"valid request allows", `{"command":"open","risk_class":"low","domain":"x.com"}`, allow, "low risk class"},
		{"valid critical request", `{"command":"curl","risk_class":"critical","domain":"localhost"}`, allow, "allowlisted domain"},
		{"unknown fields ignored", `{"command":"open","risk_class":"low","extra":42}`, allow, "low risk class"},
		{"malformed json denies", `{"command":`, deny, "decide: parse request"},
		{"empty input denies", ``, deny, "decide: empty request"},
		{"whitespace input denies", "  \n\t", deny, "decide: empty request"},
		{"missing command denies", `{"risk_class":"low"}`, deny, "decide: missing command"},
		{"json array denies", `[1,2]`, deny, "decide: parse request"},
		{"json string denies", `"hello"`, deny, "decide: parse request"},
		{"null request denies missing command", `null`, deny, "decide: missing command"},
		{"expired deadline denies", `{"command":"open","risk_class":"low","deadline":"2026-08-06T11:00:00Z"}`, deny, "decide: request deadline expired"},
		{"future deadline evaluates", `{"command":"open","risk_class":"low","deadline":"2026-08-06T13:00:00Z"}`, allow, "low risk class"},
		{"malformed deadline denies", `{"command":"open","risk_class":"low","deadline":"not-a-time"}`, deny, "decide: parse request"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			req, got, reason := decideRequest(strings.NewReader(tt.input), now)
			if got != tt.want {
				t.Errorf("decideRequest() decision = %q, want %q", got, tt.want)
			}
			if !strings.Contains(reason, tt.wantRegex) {
				t.Errorf("decideRequest() reason = %q, want substring %q", reason, tt.wantRegex)
			}
			_ = req
		})
	}
}

type fakeSink struct {
	records []audit.ExternalDecision
	err     error
}

func (f *fakeSink) Write(r audit.ExternalDecision) error {
	if f.err != nil {
		return f.err
	}
	f.records = append(f.records, r)
	return nil
}

type errWriter struct{ err error }

func (w errWriter) Write([]byte) (int, error) { return 0, w.err }

func TestRun(t *testing.T) {
	tests := []struct {
		name        string
		args        []string
		input       string
		sinkErr     error
		wantCode    int
		want        outcome
		wantRegex   string // substring match on the reason
		wantRecords int
	}{
		{"allow decision", nil, `{"command":"open","risk_class":"low"}`, nil, 0, allow, "low risk class", 1},
		{"confirm decision", nil, `{"command":"shell","risk_class":"high"}`, nil, 0, confirm, "high risk class requires confirmation", 1},
		{"deny decision", nil, `{"command":"shell","risk_class":"high","warnings":["sudo"]}`, nil, 0, deny, "high risk class with warnings: sudo", 1},
		{"malformed json fails closed", nil, `{`, nil, 0, deny, "decide: parse request", 1},
		{"empty input fails closed", nil, ``, nil, 0, deny, "decide: empty request", 1},
		{"sink failure flips allow to deny", nil, `{"command":"open","risk_class":"low"}`, errors.New("disk full"), 0, deny, "audit: write decision record: disk full", 0},
		{"sink failure keeps policy deny reason", nil, `{"command":"shell","risk_class":"high","warnings":["sudo"]}`, errors.New("disk full"), 0, deny, "high risk class with warnings: sudo", 0},
		{"help exits zero without reading stdin", []string{"--help"}, ``, nil, 0, "", "Usage:", 0},
		{"response write failure exits one", nil, `{"command":"open","risk_class":"low"}`, nil, 1, "", "", 1},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			sink := &fakeSink{err: tt.sinkErr}
			var out strings.Builder
			var w io.Writer = &out
			if tt.wantCode == 1 {
				w = errWriter{err: errors.New("pipe closed")}
			}
			code := Run(tt.args, strings.NewReader(tt.input), w, sink)
			if code != tt.wantCode {
				t.Errorf("Run() exit code = %d, want %d", code, tt.wantCode)
			}
			if len(sink.records) != tt.wantRecords {
				t.Errorf("Run() wrote %d audit records, want %d", len(sink.records), tt.wantRecords)
			}
			if tt.want == "" {
				return
			}
			var resp response
			if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
				t.Fatalf("response is not valid JSON: %v (%q)", err, out.String())
			}
			if resp.Decision != string(tt.want) {
				t.Errorf("response decision = %q, want %q", resp.Decision, tt.want)
			}
			if !strings.Contains(resp.Reason, tt.wantRegex) {
				t.Errorf("response reason = %q, want substring %q", resp.Reason, tt.wantRegex)
			}
			if resp.Reason == "" {
				t.Error("response reason must always be present")
			}
		})
	}
}

func TestRunRecordsDecision(t *testing.T) {
	sink := &fakeSink{}
	var out strings.Builder
	input := `{"command":"git push","risk_class":"high","domain":"github.com","warnings":["force"]}`
	code := Run(nil, strings.NewReader(input), &out, sink)
	if code != 0 {
		t.Fatalf("Run() exit code = %d, want 0", code)
	}
	if len(sink.records) != 1 {
		t.Fatalf("Run() wrote %d records, want 1", len(sink.records))
	}
	rec := sink.records[0]
	if rec.Command != "git push" {
		t.Errorf("record command = %q, want %q", rec.Command, "git push")
	}
	if rec.RiskClass != "high" {
		t.Errorf("record risk class = %q, want high", rec.RiskClass)
	}
	if rec.Domain != "github.com" {
		t.Errorf("record domain = %q, want github.com", rec.Domain)
	}
	if rec.Decision != "deny" {
		t.Errorf("record decision = %q, want deny", rec.Decision)
	}
	if !strings.Contains(rec.Reason, "high risk class with warnings: force") {
		t.Errorf("record reason = %q, want warnings reason", rec.Reason)
	}
	if rec.ID == "" {
		t.Error("record ID must not be empty")
	}
	if _, err := time.Parse(time.RFC3339, rec.DecidedAt); err != nil {
		t.Errorf("record DecidedAt %q is not RFC 3339: %v", rec.DecidedAt, err)
	}
}

func TestRunDefaultSinkWritesFile(t *testing.T) {
	logPath := filepath.Join(t.TempDir(), "audit.log")
	var out strings.Builder
	code := Run(nil, strings.NewReader(`{"command":"open","risk_class":"low"}`), &out, NewFileSink(logPath))
	if code != 0 {
		t.Fatalf("Run() exit code = %d, want 0", code)
	}
	data, err := os.ReadFile(logPath)
	if err != nil {
		t.Fatalf("read audit log: %v", err)
	}
	var rec audit.ExternalDecision
	if err := json.Unmarshal(bytesTrim(data), &rec); err != nil {
		t.Fatalf("audit log line is not JSON: %v (%q)", err, data)
	}
	if rec.Decision != "allow" || rec.Command != "open" {
		t.Errorf("audit log record = %+v, want allow/open", rec)
	}
	if info, err := os.Stat(logPath); err == nil && info.Mode().Perm() != 0600 {
		t.Errorf("audit log mode = %v, want 0600", info.Mode().Perm())
	}
}

func bytesTrim(b []byte) []byte {
	return []byte(strings.TrimSpace(string(b)))
}

func TestRunDefaultSinkFailsClosedWhenAuditDirUnwritable(t *testing.T) {
	// A path whose parent is a regular file cannot be created; the sink
	// fails and a would-be allow must flip to deny.
	parent := filepath.Join(t.TempDir(), "not-a-dir")
	if err := os.WriteFile(parent, []byte("x"), 0600); err != nil {
		t.Fatal(err)
	}
	var out strings.Builder
	code := Run(nil, strings.NewReader(`{"command":"open","risk_class":"low"}`), &out, NewFileSink(filepath.Join(parent, "audit.log")))
	if code != 0 {
		t.Fatalf("Run() exit code = %d, want 0", code)
	}
	var resp response
	if err := json.Unmarshal([]byte(out.String()), &resp); err != nil {
		t.Fatalf("response is not valid JSON: %v", err)
	}
	if resp.Decision != "deny" {
		t.Errorf("response decision = %q, want deny (fail closed)", resp.Decision)
	}
	if !strings.Contains(resp.Reason, "audit") {
		t.Errorf("response reason = %q, want audit failure mention", resp.Reason)
	}
}

func TestDefaultAuditPath(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("XDG_DATA_HOME", dir)
	if got := defaultAuditPath(); got != filepath.Join(dir, "symguard", "audit.log") {
		t.Errorf("defaultAuditPath() = %q, want %q", got, filepath.Join(dir, "symguard", "audit.log"))
	}
	t.Setenv("XDG_DATA_HOME", "")
	if got := defaultAuditPath(); !strings.HasSuffix(got, filepath.Join("symguard", "audit.log")) {
		t.Errorf("defaultAuditPath() = %q, want symguard/audit.log suffix", got)
	}
}

func TestPrintUsage(t *testing.T) {
	var out strings.Builder
	printUsage(&out)
	for _, want := range []string{"symguard decide", "request:", "response:", "allow|confirm|deny", "deny"} {
		if !strings.Contains(out.String(), want) {
			t.Errorf("usage missing %q", want)
		}
	}
}

func TestRecord(t *testing.T) {
	rec := record(request{Command: "open", RiskClass: "low", Domain: "x.com", Warnings: []string{"w"}}, allow, "low risk class, no warnings")
	if rec.Decision != "allow" || rec.Reason != "low risk class, no warnings" {
		t.Errorf("record = %+v, want allow with reason", rec)
	}
	if !strings.HasPrefix(rec.ID, "evt_1_decide_") {
		t.Errorf("record ID = %q, want evt_1_decide_ prefix", rec.ID)
	}
	if _, err := time.Parse(time.RFC3339, rec.DecidedAt); err != nil {
		t.Errorf("DecidedAt %q is not RFC 3339: %v", rec.DecidedAt, err)
	}
}
