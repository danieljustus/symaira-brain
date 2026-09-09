package policy

import (
	"fmt"
	"sort"
)

// MarginalCapabilityReason returns the stable explanation used when a
// capability is fully covered by one the caller already holds. Empty means
// the capability is unknown or still adds a capability.
func MarginalCapabilityReason(toolCap string, alreadyAllowed map[string]bool) string {
	allowed := make([]string, 0, len(alreadyAllowed))
	for cap, ok := range alreadyAllowed {
		if ok {
			allowed = append(allowed, cap)
		}
	}
	sort.Strings(allowed)
	for _, allowedCap := range allowed {
		if MarginalCapabilityCheck(toolCap, map[string]bool{allowedCap: true}) {
			return fmt.Sprintf("no marginal capability over already-allowed tool: %s", allowedCap)
		}
	}
	return ""
}

// ClassifyRiskWithReason returns the risk and the deterministic explanation
// for a marginal-capability cap. Static classification has no extra reason.
func ClassifyRiskWithReason(capability string, marginal bool, alreadyAllowed map[string]bool) (RiskLevel, string) {
	level := ClassifyRisk(capability, marginal)
	if !marginal {
		return level, ""
	}
	return level, MarginalCapabilityReason(capability, alreadyAllowed)
}

// String renders the risk level using the wire spelling.
func (r RiskLevel) String() string {
	switch r {
	case RiskUnknown:
		return "unknown"
	case RiskLow:
		return "low"
	case RiskMedium:
		return "medium"
	case RiskHigh:
		return "high"
	case RiskCritical:
		return "critical"
	default:
		return fmt.Sprintf("risk(%d)", r)
	}
}
