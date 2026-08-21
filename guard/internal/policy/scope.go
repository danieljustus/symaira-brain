package policy

import (
	"fmt"

	"github.com/danieljustus/symaira-guard/internal/model"
)

// wildcardScope is the capability-package ScopeAll spelling. It is kept
// local because policy must not import capability (archguard planes); the
// two constants must stay in sync.
const wildcardScope = "*"

// ScopeCeiling narrows a policy Result to the intersection of a capability
// token's granted scope and what the identity's normal policy already
// allows: a token can only ever be narrower than the human it acts for,
// never wider. When the capability being decided is not granted by the
// token scope (and the scope is not the wildcard "*"), the result is
// narrowed to a deny. An already-denied result passes through unchanged.
//
// The check is intentionally a separate function from Evaluate: it is a
// capability-layer ceiling applied after the identity's normal policy
// evaluation. The control-plane deny-list (capability.DenyControlPlane /
// capability.InScope) is enforced in the capability layer; callers apply
// it together with this ceiling.
func ScopeCeiling(tokenScope []string, capability string, result Result) Result {
	if result.Decision == model.DecisionDeny {
		return result // already the most restrictive outcome
	}
	if scopeAllows(tokenScope, capability) {
		return result
	}
	return Result{
		Decision: model.DecisionDeny,
		Reason:   fmt.Sprintf("capability %q not granted by token scope", capability),
		Matched:  false,
	}
}

// scopeAllows reports whether capability is granted by the token scope.
// The wildcard grants everything; an empty scope grants nothing (fail
// closed); an empty capability is never in scope.
func scopeAllows(scope []string, capability string) bool {
	if capability == "" {
		return false
	}
	for _, s := range scope {
		if s == wildcardScope || s == capability {
			return true
		}
	}
	return false
}
