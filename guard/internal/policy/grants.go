package policy

import (
	"time"

	"github.com/danieljustus/symaira-brain/guard/internal/grant"
	"github.com/danieljustus/symaira-brain/guard/internal/model"
)

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
	if subject == "" || lookup == nil {
		return res
	}
	grants := lookup.ActiveForSubject(subject)
	now := time.Now()
	for _, standing := range grants {
		// Treat the lookup result as untrusted: a buggy or shared lookup
		// implementation must not let another subject's grant upgrade this call.
		if standing == nil || standing.Subject != subject {
			continue
		}
		if standing.Authorizes(call.Capability, call.Purpose, call.Resource, call.Scope, now) {
			return Result{
				Rule:       res.Rule,
				Decision:   model.DecisionAllow,
				Reason:     "covered by standing grant",
				Matched:    res.Matched,
				Precedence: res.Precedence,
			}
		}
	}
	return res
}
