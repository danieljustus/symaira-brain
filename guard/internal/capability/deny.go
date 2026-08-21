package capability

import "strings"

// ControlPlanePrefixes identifies symguard's own management operations.
// No capability token — not even one with scope ScopeAll — may ever reach
// a control-plane target. Each entry is a target prefix; targets are tool
// or command identifiers such as "symguard:config:set" or
// "symguard:grant:revoke".
var ControlPlanePrefixes = []string{
	"symguard:config:",     // config mutation
	"symguard:grant:",      // grant management
	"symguard:policy:",     // policy rule mutation
	"symguard:token:",      // token minting/revocation
	"symguard:audit:",      // audit chain tampering/inspection
	"symguard:identity:",   // identity management
	"symguard:capability:", // capability layer administration
}

// DenyControlPlane reports whether target belongs to the control plane and
// is therefore unconditionally unreachable by capability tokens.
func DenyControlPlane(target string) bool {
	for _, p := range ControlPlanePrefixes {
		if strings.HasPrefix(target, p) {
			return true
		}
	}
	return false
}

// InScope reports whether target is granted by scope. The wildcard
// ScopeAll grants everything except control-plane targets, which are
// denied unconditionally (DenyControlPlane) — even when the scope names
// them explicitly. An empty scope or an empty target grants nothing
// (fail closed).
func InScope(scope []string, target string) bool {
	if target == "" || DenyControlPlane(target) {
		return false
	}
	for _, s := range scope {
		if s == ScopeAll || s == target {
			return true
		}
	}
	return false
}
