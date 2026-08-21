// Package policy defines the versioned rule catalog and evaluation engine
// for symguard's policy subsystem, including marginal-capability risk
// capping to reduce ask-prompt fatigue.
//
// # Evaluation model
//
// Rules are evaluated in three buckets derived from each rule's decision,
// in fixed order:
//
//  1. deny — any matching deny rule wins, regardless of its position or
//     precedence relative to any other rule. A deny rule whose match
//     criteria cannot be evaluated also resolves to denied (defensive
//     deny): an evaluation error never resolves to "not denied".
//  2. require — every require rule must hold (match) for the call to be
//     allowed. The first require rule that does not hold denies the call;
//     an unevaluable require rule also denies it.
//  3. allow — when no deny matched and every require rule holds, the first
//     matching rule in precedence order decides. This bucket holds every
//     non-deny, non-require decision (allow, ask, redact, readonly,
//     sandbox) and returns the matched rule's own decision.
//
// Precedence orders rules within a bucket and is reported in the Result;
// it never lets one rule override a deny from another bucket.
//
// # Composition
//
// Catalogs compose by set concatenation (Merge). Because deny wins and
// require rules must all hold, merging can never weaken the result: a
// deny or requirement in either catalog keeps its force in the merged
// catalog, and an allow can never erase a deny.
//
// Marginal-capability rule: after the static capability-name lookup, cap the
// resulting risk when a tool grants zero marginal capability — the calling
// agent/client could already achieve the same effect through another tool
// already resolved to allow, or through a trust boundary it already sits
// inside. This only ever caps downward, never up.
package policy

import (
	"fmt"
	"strings"

	"github.com/danieljustus/symaira-guard/internal/grant"
	"github.com/danieljustus/symaira-guard/internal/model"
)

// RuleID is a stable, unique rule identifier.
type RuleID string

// Version is a semver-compatible rule version.
type Version string

// Precedence defines rule ordering (lower = evaluated first).
type Precedence int

// Rule is a single policy rule with stable identity and deterministic ordering.
type Rule struct {
	ID          RuleID         `json:"id"`
	Version     Version        `json:"version"`
	Precedence  Precedence     `json:"precedence"`
	Decision    model.Decision `json:"decision"`
	Match       MatchCriteria  `json:"match"`
	Reason      string         `json:"reason,omitempty"`
	ObserveOnly bool           `json:"observe_only,omitempty"`
}

// MatchCriteria defines what a rule matches against.
// All non-empty fields must match (AND semantics).
type MatchCriteria struct {
	Server          string   `json:"server,omitempty"`
	Tool            string   `json:"tool,omitempty"`
	Capability      string   `json:"capability,omitempty"`
	Remote          string   `json:"remote,omitempty"`
	CommandContains []string `json:"command_contains,omitempty"`
}

// Bucket identifies the evaluation bucket a rule belongs to.
type Bucket string

const (
	// BucketDeny holds deny rules; any match resolves to denied, and an
	// unevaluable match is treated as denied (defensive deny).
	BucketDeny Bucket = "deny"
	// BucketRequire holds require rules; every rule must hold (match) for
	// the call to be allowed.
	BucketRequire Bucket = "require"
	// BucketAllow holds every non-deny, non-require decision (allow, ask,
	// redact, readonly, sandbox); the first matching rule in precedence
	// order decides.
	BucketAllow Bucket = "allow"
)

// Result is the structured outcome of a policy evaluation.
type Result struct {
	Rule       *Rule                  `json:"rule,omitempty"`
	Decision   model.Decision         `json:"decision"`
	Reason     string                 `json:"reason,omitempty"`
	Matched    bool                   `json:"matched"`
	Precedence Precedence             `json:"precedence,omitempty"`
	Bucket     Bucket                 `json:"bucket,omitempty"`
	Trace      []model.RuleTraceEntry `json:"trace,omitempty"` // diagnostic only; populated only when Options.Trace is set
}

// Options controls catalog evaluation.
type Options struct {
	// Trace records one entry per rule in Result.Trace, in catalog order,
	// with the rule's matched state, decision, and bucket. Diagnostic
	// only: tracing never changes the decision. Off by default.
	Trace bool
}

// Catalog is a versioned, validated collection of rules.
type Catalog struct {
	Rules   []Rule  `json:"rules"`
	Version Version `json:"version"`
}

// Evaluate checks a tool call against the catalog and returns the policy
// outcome. Rules are evaluated in three buckets in fixed order — deny,
// require, allow (see the package doc for the full semantics): any
// matching deny wins regardless of position or precedence; every require
// rule must hold; otherwise the first matching rule in the allow bucket
// (or the provided default) decides. Tracing is off; use EvaluateOpts
// with Options{Trace: true} to collect the per-rule trace.
func (c *Catalog) Evaluate(call model.ToolCall, defaults model.Decision) Result {
	return c.EvaluateOpts(call, defaults, Options{})
}

// EvaluateOpts is Evaluate with explicit options. When opts.Trace is set,
// Result.Trace lists every rule in the catalog exactly once, in catalog
// order, with its matched state, decision, and bucket. Tracing is
// diagnostic only and never changes the decision.
func (c *Catalog) EvaluateOpts(call model.ToolCall, defaults model.Decision, opts Options) Result {
	var trace []model.RuleTraceEntry
	if opts.Trace {
		trace = make([]model.RuleTraceEntry, 0, len(c.Rules))
	}
	var (
		denyRule   *Rule // first matching deny in precedence order
		denyErr    *Rule // first deny that could not be evaluated
		requireBad *Rule // first require rule that did not hold
		requireErr *Rule // first require rule that could not be evaluated
		allowMatch *Rule // first matching allow-bucket rule
	)
	for i := range c.Rules {
		r := &c.Rules[i]
		bucket := bucketOf(r.Decision)
		matched, err := matches(r.Match, call)
		if opts.Trace {
			trace = append(trace, model.RuleTraceEntry{
				RuleID:   string(r.ID),
				Matched:  matched,
				Decision: r.Decision,
				Bucket:   string(bucket),
			})
		}
		switch bucket {
		case BucketDeny:
			switch {
			case err != nil:
				// Defensive deny: an unevaluable deny rule never resolves
				// to "not denied".
				if denyErr == nil {
					denyErr = r
				}
			case matched:
				if denyRule == nil {
					denyRule = r
				}
			}
		case BucketRequire:
			switch {
			case err != nil:
				if requireErr == nil {
					requireErr = r
				}
			case !matched:
				if requireBad == nil {
					requireBad = r
				}
			}
		case BucketAllow:
			if err != nil {
				// An unevaluable allow rule never grants; skip it.
				continue
			}
			if matched && allowMatch == nil {
				allowMatch = r
			}
		}
	}

	switch {
	case denyRule != nil:
		return Result{
			Rule:       denyRule,
			Decision:   model.DecisionDeny,
			Reason:     denyRule.Reason,
			Matched:    true,
			Precedence: denyRule.Precedence,
			Bucket:     BucketDeny,
			Trace:      trace,
		}
	case denyErr != nil:
		return Result{
			Rule:       denyErr,
			Decision:   model.DecisionDeny,
			Reason:     fmt.Sprintf("deny rule %q could not be evaluated", denyErr.ID),
			Matched:    false,
			Precedence: denyErr.Precedence,
			Bucket:     BucketDeny,
			Trace:      trace,
		}
	case requireBad != nil:
		return Result{
			Rule:       requireBad,
			Decision:   model.DecisionDeny,
			Reason:     fmt.Sprintf("require rule %q did not hold", requireBad.ID),
			Matched:    false,
			Precedence: requireBad.Precedence,
			Bucket:     BucketRequire,
			Trace:      trace,
		}
	case requireErr != nil:
		return Result{
			Rule:       requireErr,
			Decision:   model.DecisionDeny,
			Reason:     fmt.Sprintf("require rule %q could not be evaluated", requireErr.ID),
			Matched:    false,
			Precedence: requireErr.Precedence,
			Bucket:     BucketRequire,
			Trace:      trace,
		}
	case allowMatch != nil:
		return Result{
			Rule:       allowMatch,
			Decision:   allowMatch.Decision,
			Reason:     allowMatch.Reason,
			Matched:    true,
			Precedence: allowMatch.Precedence,
			Bucket:     BucketAllow,
			Trace:      trace,
		}
	default:
		return Result{
			Decision: defaults,
			Reason:   "default capability decision",
			Matched:  false,
			Trace:    trace,
		}
	}
}

// bucketOf derives the evaluation bucket from a rule's decision.
func bucketOf(d model.Decision) Bucket {
	switch d {
	case model.DecisionDeny:
		return BucketDeny
	case model.DecisionRequire:
		return BucketRequire
	default:
		return BucketAllow // allow, ask, redact, readonly, sandbox
	}
}

// GrantLookup reports the active grants held by a subject. It is the policy
// engine's read seam into the grant store (internal/grant).
type GrantLookup interface {
	ActiveForSubject(subject string) []*grant.Grant
}

// EvaluateWithGrants evaluates a tool call and consults the grant store:
// when the static result would ask the human and the subject holds at least
// one active grant, the decision is upgraded to allow. A standing grant only
// ever upgrades Ask — it never overrides an explicit decision such as deny,
// redact, or sandbox.
func (c *Catalog) EvaluateWithGrants(subject string, call model.ToolCall, defaults model.Decision, lookup GrantLookup) Result {
	res := c.Evaluate(call, defaults)
	if res.Decision != model.DecisionAsk {
		return res
	}
	if subject == "" || lookup == nil || len(lookup.ActiveForSubject(subject)) == 0 {
		return res
	}
	return Result{
		Rule:       res.Rule,
		Decision:   model.DecisionAllow,
		Reason:     "covered by standing grant",
		Matched:    res.Matched,
		Precedence: res.Precedence,
	}
}

// matches checks whether a tool call satisfies the match criteria.
// It returns an error when the criteria cannot be evaluated against the
// call (for example CommandContains against non-string args). Callers
// must treat an error on a deny or require rule as fail-closed: it never
// resolves to "not denied" or "holding".
func matches(m MatchCriteria, call model.ToolCall) (bool, error) {
	if m.Server != "" && m.Server != call.Server {
		return false, nil
	}
	if m.Tool != "" && m.Tool != call.Tool {
		return false, nil
	}
	if m.Capability != "" && m.Capability != call.Capability {
		return false, nil
	}
	if len(m.CommandContains) > 0 {
		args, ok := call.Args.(string)
		if !ok {
			return false, fmt.Errorf("command_contains match requires string args, got %T", call.Args)
		}
		matched := false
		for _, substr := range m.CommandContains {
			if strings.Contains(args, substr) {
				matched = true
				break
			}
		}
		if !matched {
			return false, nil
		}
	}
	return true, nil
}

// Validate checks the catalog for structural errors.
func (c *Catalog) Validate() error {
	seen := make(map[RuleID]bool)
	prevPrecedence := Precedence(-1)

	for i, rule := range c.Rules {
		if rule.ID == "" {
			return fmt.Errorf("rules[%d]: empty rule ID", i)
		}
		if seen[rule.ID] {
			return fmt.Errorf("rules[%d]: duplicate rule ID %q", i, rule.ID)
		}
		seen[rule.ID] = true

		if err := model.ValidateDecision(rule.Decision); err != nil {
			return fmt.Errorf("rules[%d] %q: %w", i, rule.ID, err)
		}

		if rule.Match.Server == "" && rule.Match.Tool == "" &&
			rule.Match.Capability == "" && rule.Match.Remote == "" &&
			len(rule.Match.CommandContains) == 0 {
			return fmt.Errorf("rules[%d] %q: match must specify at least one criterion", i, rule.ID)
		}

		if rule.Precedence <= prevPrecedence {
			return fmt.Errorf("rules[%d] %q: precedence %d not strictly greater than previous %d",
				i, rule.ID, rule.Precedence, prevPrecedence)
		}
		prevPrecedence = rule.Precedence
	}
	return nil
}

// NewCatalog creates a validated catalog from rules.
func NewCatalog(rules []Rule, version Version) (*Catalog, error) {
	c := &Catalog{Rules: rules, Version: version}
	if err := c.Validate(); err != nil {
		return nil, err
	}
	return c, nil
}

// Merge returns a new catalog whose rules are the set concatenation of
// the receiver's rules followed by other's rules. Because evaluation is
// bucketed, merging can never weaken the result: a deny or require rule
// in either catalog keeps its force in the merged catalog, and an allow
// can never erase a deny. Precedence is renumbered 1..N in concatenation
// order (receiver first), which preserves the ordering within each bucket
// while keeping the merged catalog valid. The receiver's Version is kept;
// callers are responsible for versioning the merged catalog. It returns
// an error when the merged catalog is invalid, for example when a rule ID
// appears in both catalogs.
func (c *Catalog) Merge(other *Catalog) (*Catalog, error) {
	merged := make([]Rule, 0, len(c.Rules)+len(other.Rules))
	merged = append(merged, c.Rules...)
	merged = append(merged, other.Rules...)
	for i := range merged {
		merged[i].Precedence = Precedence(i + 1)
	}
	return NewCatalog(merged, c.Version)
}
