package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/guard/cmd/symguard/grants"
	"github.com/danieljustus/symaira-brain/guard/internal/approval"
	"github.com/danieljustus/symaira-brain/guard/internal/grant"
	"github.com/danieljustus/symaira-brain/guard/internal/model"
	"github.com/danieljustus/symaira-brain/guard/internal/policy"
)

// SEC-002 cases: grant validation/authorization, approval->grant
// conversion, the policy grant consult, and `symguard grants` CLI bytes
// (stdout plus the persisted grants.json contents).
//
// Determinism notes:
//   - grant.Authorizes takes an injected instant, so its corpus is exact.
//   - approval.Decision.Grant and policy.EvaluateWithGrants read the wall
//     clock internally; the corpus only pins expiry outcomes that cannot
//     flip (expiry 2000 = always expired, 2027/2999 = never), and the
//     approval corpus always carries a non-zero DecidedAt so Go's
//     time.Now() fallback is never reached. Grant IDs minted from
//     wall-clock entropy (grant.NewID) are replaced with a fixture ID
//     after the real conversion; every other field is production output.
//   - CLI cases run against a throwaway SYMGUARD_DATA directory; the
//     store path is normalized to "<store-dir>" in captured bytes, so
//     fixture bytes are independent of TMPDIR.

const storeDirPlaceholder = "<store-dir>"

type grantAuthorizesInput struct {
	Grant      *grant.Grant `json:"grant"`
	Capability string       `json:"capability"`
	Purpose    string       `json:"purpose"`
	Resource   string       `json:"resource"`
	Scope      string       `json:"scope"`
	Now        string       `json:"now"`
}

type approvalGrantInput struct {
	Decision approval.Decision `json:"decision"`
	Subject  string            `json:"subject"`
	GrantID  string            `json:"grant_id"`
}

type grantsEvalInput struct {
	Rules   []policy.Rule  `json:"rules"`
	Call    model.ToolCall `json:"call"`
	Default model.Decision `json:"default"`
	Subject string         `json:"subject"`
	Grants  []*grant.Grant `json:"grants"`
}

type grantsCliInput struct {
	Args  []string `json:"args"`
	Store string   `json:"store"` // "" means no grants.json file exists
}

type grantsCliOutput struct {
	Stdout      string `json:"stdout"`
	StoreExists bool   `json:"store_exists"`
	Store       string `json:"store,omitempty"`
}

// rawLookup hands EvaluateWithGrants the grants unfiltered, pinning that
// the engine itself skips nil and foreign-subject entries.
type rawLookup []*grant.Grant

func (l rawLookup) ActiveForSubject(string) []*grant.Grant { return l }

func sec002GrantsCases() []oracleCase {
	var cases []oracleCase
	cases = append(cases, sec002AuthorizesCases()...)
	cases = append(cases, sec002AddValidateCases()...)
	cases = append(cases, sec002ApprovalCases()...)
	cases = append(cases, sec002EvalWithGrantsCases()...)
	cases = append(cases, sec002GrantsCliCases()...)
	return cases
}

func baseBoundGrant() *grant.Grant {
	return &grant.Grant{
		ID:           "g1",
		Scope:        grant.ScopeSession,
		Origin:       grant.Origin{Epoch: 1722924000, Via: "approval"},
		GrantedAt:    time.Date(2026, 8, 6, 10, 0, 0, 0, time.UTC),
		Subject:      "agent-1",
		Capability:   "read_private",
		Purpose:      "oracle-purpose",
		Resource:     "fs/read_file",
		ScopeCeiling: []string{"session"},
		ExpiresAt:    time.Date(2027, 1, 1, 0, 0, 0, 0, time.UTC),
	}
}

func sec002AuthorizesCases() []oracleCase {
	const defaultNow = "2026-08-06T12:00:00Z"
	type tc struct {
		id         string
		mutate     func(*grant.Grant)
		capability string
		purpose    string
		resource   string
		scope      string
		now        string
		nilGrant   bool
	}
	table := []tc{
		{id: "grant_authorizes_exact_scope", capability: "read_private", purpose: "oracle-purpose", resource: "fs/read_file", scope: "session", now: defaultNow},
		{id: "grant_authorizes_wildcard_ceiling", mutate: func(g *grant.Grant) { g.ScopeCeiling = []string{"*"} }, capability: "read_private", purpose: "oracle-purpose", resource: "fs/read_file", scope: "device", now: defaultNow},
		{id: "grant_authorizes_ceiling_mismatch", mutate: func(g *grant.Grant) { g.ScopeCeiling = []string{"vault"} }, capability: "read_private", purpose: "oracle-purpose", resource: "fs/read_file", scope: "session", now: defaultNow},
		{id: "grant_authorizes_capability_mismatch", capability: "read_secret", purpose: "oracle-purpose", resource: "fs/read_file", scope: "session", now: defaultNow},
		{id: "grant_authorizes_purpose_mismatch", capability: "read_private", purpose: "other-purpose", resource: "fs/read_file", scope: "session", now: defaultNow},
		{id: "grant_authorizes_resource_mismatch", capability: "read_private", purpose: "oracle-purpose", resource: "fs/write_file", scope: "session", now: defaultNow},
		{id: "grant_authorizes_empty_call_scope", capability: "read_private", purpose: "oracle-purpose", resource: "fs/read_file", scope: "", now: defaultNow},
		{id: "grant_authorizes_expired_exact", capability: "read_private", purpose: "oracle-purpose", resource: "fs/read_file", scope: "session", now: "2027-01-01T00:00:00Z"},
		{id: "grant_authorizes_revoked", mutate: func(g *grant.Grant) { g.Revoked = true }, capability: "read_private", purpose: "oracle-purpose", resource: "fs/read_file", scope: "session", now: defaultNow},
		{id: "grant_authorizes_unknown_grant_scope", mutate: func(g *grant.Grant) { g.Scope = "cluster" }, capability: "read_private", purpose: "oracle-purpose", resource: "fs/read_file", scope: "session", now: defaultNow},
		{id: "grant_authorizes_missing_capability", mutate: func(g *grant.Grant) { g.Capability = "" }, capability: "read_private", purpose: "oracle-purpose", resource: "fs/read_file", scope: "session", now: defaultNow},
		{id: "grant_authorizes_missing_purpose", mutate: func(g *grant.Grant) { g.Purpose = "" }, capability: "read_private", purpose: "oracle-purpose", resource: "fs/read_file", scope: "session", now: defaultNow},
		{id: "grant_authorizes_missing_resource", mutate: func(g *grant.Grant) { g.Resource = "" }, capability: "read_private", purpose: "oracle-purpose", resource: "fs/read_file", scope: "session", now: defaultNow},
		{id: "grant_authorizes_missing_expiry", mutate: func(g *grant.Grant) { g.ExpiresAt = time.Time{} }, capability: "read_private", purpose: "oracle-purpose", resource: "fs/read_file", scope: "session", now: defaultNow},
		{id: "grant_authorizes_empty_ceiling_entry", mutate: func(g *grant.Grant) { g.ScopeCeiling = []string{""} }, capability: "read_private", purpose: "oracle-purpose", resource: "fs/read_file", scope: "session", now: defaultNow},
		{id: "grant_authorizes_missing_ceiling", mutate: func(g *grant.Grant) { g.ScopeCeiling = nil }, capability: "read_private", purpose: "oracle-purpose", resource: "fs/read_file", scope: "session", now: defaultNow},
		{id: "grant_authorizes_nil_grant", nilGrant: true, capability: "read_private", purpose: "oracle-purpose", resource: "fs/read_file", scope: "session", now: defaultNow},
		{id: "grant_authorizes_offset_expiry", mutate: func(g *grant.Grant) { g.ExpiresAt = parseTime("2027-01-01T01:00:00+01:00") }, capability: "read_private", purpose: "oracle-purpose", resource: "fs/read_file", scope: "session", now: defaultNow},
	}
	cases := make([]oracleCase, 0, len(table))
	for _, tc := range table {
		g := baseBoundGrant()
		if tc.mutate != nil {
			tc.mutate(g)
		}
		var ptr *grant.Grant = g
		if tc.nilGrant {
			ptr = nil
		}
		input := grantAuthorizesInput{
			Grant:      ptr,
			Capability: tc.capability,
			Purpose:    tc.purpose,
			Resource:   tc.resource,
			Scope:      tc.scope,
			Now:        tc.now,
		}
		authorized := ptr.Authorizes(tc.capability, tc.purpose, tc.resource, tc.scope, parseTime(tc.now))
		cases = append(cases, successCase(tc.id, "grant_authorizes", input, authorized))
	}
	return cases
}

func sec002AddValidateCases() []oracleCase {
	withID := func(g *grant.Grant, id string) *grant.Grant {
		g.ID = id
		return g
	}
	table := []struct {
		id string
		g  *grant.Grant
	}{
		{"grant_add_valid_session", withID(baseBoundGrant(), "add-1")},
		{"grant_add_valid_device", func() *grant.Grant {
			g := baseBoundGrant()
			g.ID = "add-2"
			g.Scope = grant.ScopeDevice
			g.ScopeCeiling = []string{"session", "device"}
			return g
		}()},
		{"grant_add_nil", nil},
		{"grant_add_empty_id", func() *grant.Grant { g := baseBoundGrant(); g.ID = ""; return g }()},
		{"grant_add_unknown_scope", func() *grant.Grant { g := baseBoundGrant(); g.Scope = "cluster"; return g }()},
		{"grant_add_empty_subject", func() *grant.Grant { g := baseBoundGrant(); g.Subject = ""; return g }()},
		{"grant_add_incomplete_binding", func() *grant.Grant { g := baseBoundGrant(); g.Capability = ""; return g }()},
	}
	cases := make([]oracleCase, 0, len(table))
	for _, tc := range table {
		dir, err := os.MkdirTemp("", "sec002-grant-add-")
		if err != nil {
			panic(err)
		}
		store, oerr := grant.Open(dir)
		if oerr != nil {
			panic(oerr)
		}
		aerr := store.Add(tc.g)
		if rerr := os.RemoveAll(dir); rerr != nil {
			panic(rerr)
		}
		if aerr != nil {
			cases = append(cases, errorCase(tc.id, "grant_add_validate", tc.g, aerr))
			continue
		}
		cases = append(cases, successCase(tc.id, "grant_add_validate", tc.g, true))
	}
	return cases
}

func sec002ApprovalCases() []oracleCase {
	baseDecision := func() approval.Decision {
		return approval.Decision{
			ID:           "apr-1",
			Approved:     true,
			TTL:          time.Hour,
			DecidedAt:    parseTime("2026-08-06T12:00:00Z"),
			Capability:   "read_secret",
			Purpose:      "oracle-purpose",
			Resource:     "fs/read_file",
			ScopeCeiling: []string{"session"},
		}
	}
	table := []struct {
		id      string
		dec     approval.Decision
		subject string
		grantID string
	}{
		{"approval_grant_converts", baseDecision(), "agent-1", "gnt_injected"},
		{"approval_grant_zero_ttl", func() approval.Decision { d := baseDecision(); d.TTL = 0; return d }(), "agent-1", "gnt_injected"},
		{"approval_grant_negative_ttl", func() approval.Decision { d := baseDecision(); d.TTL = -time.Second; return d }(), "agent-1", "gnt_injected"},
		{"approval_grant_unapproved", func() approval.Decision { d := baseDecision(); d.Approved = false; return d }(), "agent-1", "gnt_injected"},
		{"approval_grant_empty_subject", baseDecision(), "", "gnt_injected"},
		{"approval_grant_offset_time", func() approval.Decision {
			d := baseDecision()
			d.DecidedAt = parseTime("2026-08-06T14:00:00+02:00")
			return d
		}(), "agent-1", "gnt_injected"},
		{"approval_grant_unbound_copy", func() approval.Decision {
			d := baseDecision()
			d.Capability = ""
			d.Purpose = ""
			d.Resource = ""
			d.ScopeCeiling = nil
			return d
		}(), "agent-1", "gnt_injected"},
	}
	cases := make([]oracleCase, 0, len(table))
	for _, tc := range table {
		input := approvalGrantInput{Decision: tc.dec, Subject: tc.subject, GrantID: tc.grantID}
		g, err := tc.dec.Grant(tc.subject)
		if err != nil {
			cases = append(cases, errorCase(tc.id, "approval_grant", input, err))
			continue
		}
		if g == nil {
			cases = append(cases, successCase(tc.id, "approval_grant", input, nil))
			continue
		}
		g.ID = tc.grantID
		cases = append(cases, successCase(tc.id, "approval_grant", input, g))
	}
	return cases
}

func sec002EvalWithGrantsCases() []oracleCase {
	call := model.ToolCall{
		Server:     "fs",
		Tool:       "read_file",
		Capability: "read_secret",
		Purpose:    "oracle-purpose",
		Resource:   "fs/read_file",
		Scope:      "session",
	}
	askRule := makeRule("ask-1", 10, model.DecisionAsk, policy.MatchCriteria{Capability: "read_secret"}, "read needs approval")
	denyRule := makeRule("deny-1", 10, model.DecisionDeny, policy.MatchCriteria{Capability: "read_secret"}, "secret reads blocked")
	allowRule := makeRule("allow-1", 10, model.DecisionAllow, policy.MatchCriteria{Capability: "read_secret"}, "read allowed")
	dbRule := makeRule("db-deny", 10, model.DecisionDeny, policy.MatchCriteria{Server: "db"}, "db denied")
	bound := func(id, subject, expires string, ceiling []string) *grant.Grant {
		g := baseBoundGrant()
		g.ID = id
		g.Subject = subject
		g.Capability = "read_secret"
		g.ExpiresAt = parseTime(expires)
		g.ScopeCeiling = ceiling
		return g
	}
	sessionCeiling := []string{"session"}
	upgraded := bound("g1", "agent-1", "2999-01-01T00:00:00Z", sessionCeiling)
	table := []struct {
		id      string
		rules   []policy.Rule
		call    model.ToolCall
		def     model.Decision
		subject string
		grants  []*grant.Grant
	}{
		{"policy_grant_ask_upgraded", []policy.Rule{askRule}, call, model.DecisionAsk, "agent-1", []*grant.Grant{upgraded}},
		{"policy_grant_ask_without_grant", []policy.Rule{askRule}, call, model.DecisionAsk, "agent-1", nil},
		{"policy_grant_deny_never_upgraded", []policy.Rule{denyRule}, call, model.DecisionAsk, "agent-1", []*grant.Grant{upgraded}},
		{"policy_grant_allow_unchanged", []policy.Rule{allowRule}, call, model.DecisionAsk, "agent-1", []*grant.Grant{upgraded}},
		{"policy_grant_wrong_subject_skipped", []policy.Rule{askRule}, call, model.DecisionAsk, "agent-1", []*grant.Grant{bound("g1", "agent-2", "2999-01-01T00:00:00Z", sessionCeiling)}},
		{"policy_grant_nil_entry_skipped", []policy.Rule{askRule}, call, model.DecisionAsk, "agent-1", []*grant.Grant{nil, upgraded}},
		{"policy_grant_expired_stays_ask", []policy.Rule{askRule}, call, model.DecisionAsk, "agent-1", []*grant.Grant{bound("g1", "agent-1", "2000-01-01T00:00:00Z", sessionCeiling)}},
		{"policy_grant_revoked_stays_ask", []policy.Rule{askRule}, call, model.DecisionAsk, "agent-1", func() []*grant.Grant {
			g := bound("g1", "agent-1", "2999-01-01T00:00:00Z", sessionCeiling)
			g.Revoked = true
			return []*grant.Grant{g}
		}()},
		{"policy_grant_ceiling_mismatch_stays_ask", []policy.Rule{askRule}, call, model.DecisionAsk, "agent-1", []*grant.Grant{bound("g1", "agent-1", "2999-01-01T00:00:00Z", []string{"vault"})}},
		{"policy_grant_empty_subject_stays_ask", []policy.Rule{askRule}, call, model.DecisionAsk, "", []*grant.Grant{upgraded}},
		{"policy_grant_default_ask_upgraded", []policy.Rule{dbRule}, call, model.DecisionAsk, "agent-1", []*grant.Grant{upgraded}},
	}
	cases := make([]oracleCase, 0, len(table))
	for _, tc := range table {
		catalog, err := policy.NewCatalog(tc.rules, "1.0.0")
		if err != nil {
			panic(err)
		}
		input := grantsEvalInput{
			Rules:   tc.rules,
			Call:    tc.call,
			Default: tc.def,
			Subject: tc.subject,
			Grants:  tc.grants,
		}
		result := catalog.EvaluateWithGrants(tc.subject, tc.call, tc.def, rawLookup(tc.grants))
		cases = append(cases, successCase(tc.id, "evaluate_with_grants", input, result))
	}
	return cases
}

func sec002GrantsCliCases() []oracleCase {
	const orderedStore = `[
  {"id":"gnt-old","scope":"device","origin":{"epoch":1722924000,"via":"approval"},"granted_at":"2026-08-06T10:00:00Z","subject":"agent-old","capability":"read_private","purpose":"oracle-purpose","resource":"fs/read_file","scope_ceiling":["session"],"expires_at":"2027-01-01T00:00:00Z"},
  {"id":"gnt-frac","scope":"vault","origin":{"epoch":1722924001,"via":"approval"},"granted_at":"2026-08-06T16:34:56.123456789+02:00","subject":"agent-frac","capability":"read_private","purpose":"oracle-purpose","resource":"fs/read_file","scope_ceiling":["vault"],"expires_at":"2027-06-01T00:00:00Z"}
]`
	const legacyStore = `[{"id":"legacy","scope":"device","subject":"agent-legacy","granted_at":"2026-08-06T10:00:00Z"}]`
	const revokeStore = `[{"id":"gnt-a","scope":"device","origin":{"epoch":1722924000,"via":"approval"},"granted_at":"2026-08-06T10:00:00Z","subject":"agent-a","capability":"read_private","purpose":"oracle-purpose","resource":"fs/read_file","scope_ceiling":["session"],"expires_at":"2027-01-01T00:00:00Z"},{"id":"gnt-b","scope":"vault","origin":{"epoch":1722924001,"via":"approval"},"granted_at":"2026-08-06T11:00:00Z","subject":"agent-b","capability":"read_private","purpose":"oracle-purpose","resource":"fs/read_file","scope_ceiling":["vault"],"expires_at":"2027-01-01T00:00:00Z"}]`
	table := []struct {
		id    string
		args  []string
		store string
	}{
		{"grants_cli_list_empty", []string{"list"}, ""},
		{"grants_cli_list_ordered", []string{"list"}, orderedStore},
		{"grants_cli_list_legacy", []string{"list"}, legacyStore},
		{"grants_cli_list_null_store", []string{"list"}, "null\n"},
		{"grants_cli_revoke_one", []string{"revoke", "gnt-b"}, revokeStore},
		{"grants_cli_revoke_all", []string{"revoke", "--all"}, revokeStore},
		{"grants_cli_revoke_missing", []string{"revoke", "missing"}, ""},
		{"grants_cli_revoke_missing_id", []string{"revoke"}, ""},
		{"grants_cli_revoke_all_with_id", []string{"revoke", "--all", "gnt-a"}, ""},
		{"grants_cli_revoke_unexpected_flag", []string{"revoke", "-x"}, ""},
		{"grants_cli_unknown_subcommand", []string{"frobnicate"}, ""},
		{"grants_cli_list_malformed_store", []string{"list"}, "{not json\n"},
		{"grants_cli_list_unknown_scope", []string{"list"}, `[{"id":"g1","scope":"cluster","subject":"a"}]`},
		{"grants_cli_list_duplicate_id", []string{"list"}, `[{"id":"g1","scope":"device","subject":"a"},{"id":"g1","scope":"vault","subject":"b"}]`},
		// Deferred (not pinnable with the read-only Rust grants store):
		// an empty `granted_at` reaches encoding/json's time decode error
		// in Go but goes through serde_json in Rust, which appends
		// " at line X column Y" to the custom deserialize error. Pinning
		// it would require editing rust/symbrain-cli/src/guard_grants_store.rs.
	}
	cases := make([]oracleCase, 0, len(table))
	for _, tc := range table {
		cases = append(cases, grantsCliCase(tc.id, grantsCliInput{Args: tc.args, Store: tc.store}))
	}
	return cases
}

func grantsCliCase(id string, input grantsCliInput) oracleCase {
	dir, err := os.MkdirTemp("", "sec002-grants-cli-")
	if err != nil {
		panic(err)
	}
	defer func() {
		if rerr := os.RemoveAll(dir); rerr != nil {
			panic(rerr)
		}
	}()
	storePath := filepath.Join(dir, "grants.json")
	if input.Store != "" {
		if werr := os.WriteFile(storePath, []byte(input.Store), 0o600); werr != nil {
			panic(werr)
		}
	}
	old, had := os.LookupEnv("SYMGUARD_DATA")
	if serr := os.Setenv("SYMGUARD_DATA", dir); serr != nil {
		panic(serr)
	}
	defer func() {
		if had {
			_ = os.Setenv("SYMGUARD_DATA", old)
			return
		}
		_ = os.Unsetenv("SYMGUARD_DATA")
	}()
	var out bytes.Buffer
	grants.Run(input.Args, &out)
	stdout := strings.ReplaceAll(out.String(), dir, storeDirPlaceholder)
	persisted, rerr := os.ReadFile(storePath)
	output := grantsCliOutput{Stdout: stdout, StoreExists: rerr == nil}
	if rerr == nil {
		output.Store = strings.ReplaceAll(string(persisted), dir, storeDirPlaceholder)
	}
	return successCase(id, "grants_cli", input, output)
}
