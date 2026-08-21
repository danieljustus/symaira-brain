package policy

// CapabilityEffect defines what effects a capability grants.
// This is used by marginal-capability capping: if a tool grants no
// capability beyond one the agent already holds through an already-allowed
// tool, its risk is capped to low.
type CapabilityEffect int

const (
	EffectNone CapabilityEffect = iota
	EffectReadPublic
	EffectReadPrivate
	EffectReadSecret
	EffectWriteFile
	EffectShell
	EffectNetwork
	EffectBrowser
	EffectCredentialUse
	EffectDeploy
	EffectDestructive
)

// capabilityEffects maps capability names to their effects.
var capabilityEffects = map[string]CapabilityEffect{
	"read_public":    EffectReadPublic,
	"read_private":   EffectReadPrivate,
	"read_secret":    EffectReadSecret,
	"write_file":     EffectWriteFile,
	"shell":          EffectShell,
	"network":        EffectNetwork,
	"browser":        EffectBrowser,
	"credential_use": EffectCredentialUse,
	"deploy":         EffectDeploy,
	"destructive":    EffectDestructive,
}

// supersetEffects maps each effect to a set of effects it encompasses.
// For example, shell (EffectShell) encompasses read_public, read_private,
// write_file, and network — if you can run arbitrary commands, you can
// read files, write files, and make network requests.
var supersetEffects = map[CapabilityEffect]map[CapabilityEffect]bool{
	EffectShell: {
		EffectReadPublic:  true,
		EffectReadPrivate: true,
		EffectWriteFile:   true,
		EffectNetwork:     true,
	},
	EffectNetwork: {
		EffectReadPublic: true,
	},
	EffectCredentialUse: {
		EffectReadSecret: true,
	},
	EffectDeploy: {
		EffectWriteFile: true,
		EffectNetwork:   true,
	},
	EffectDestructive: {
		EffectWriteFile: true,
		EffectDeploy:    true,
	},
}

// MarginalCapabilityCheck checks whether the tool's capability is a strict
// subset of any already-allowed capability. If so, it returns true (cap
// the risk downward). The alreadyAllowed set should contain the resolved
// decision for each capability the calling agent already holds.
//
// Example: if shell is already allowed (allow), read_file adds no marginal
// capability (shell can already cat files), so read_file's risk can be
// capped to low.
func MarginalCapabilityCheck(toolCap string, alreadyAllowed map[string]bool) bool {
	toolEffect, ok := capabilityEffects[toolCap]
	if !ok {
		return false // unknown capability, can't assess
	}

	for allowedCap := range alreadyAllowed {
		allowedEffect, ok := capabilityEffects[allowedCap]
		if !ok {
			continue
		}
		// If the allowed capability's effect encompasses the tool's effect,
		// the tool grants zero marginal capability.
		if allowedEffect == toolEffect {
			return true // same effect
		}
		if supersets, ok := supersetEffects[allowedEffect]; ok {
			if supersets[toolEffect] {
				return true // tool effect is a subset of allowed effect
			}
		}
	}

	return false
}

// RiskLevel represents a classification level for a capability.
type RiskLevel int

const (
	RiskUnknown RiskLevel = iota
	RiskLow
	RiskMedium
	RiskHigh
	RiskCritical
)

// ClassifyRisk determines the initial risk level for a capability name.
// When marginal is true (the tool adds no capability beyond what's already
// allowed), the risk is capped to Low.
func ClassifyRisk(capability string, marginal bool) RiskLevel {
	// Start with the static risk table.
	base := staticRisk(capability)

	// Marginal capping only ever reduces risk, never increases it.
	if marginal && base > RiskLow {
		return RiskLow
	}
	return base
}

// staticRisk returns the base risk level for a capability name.
func staticRisk(capability string) RiskLevel {
	switch capability {
	case "read_public":
		return RiskLow
	case "read_private":
		return RiskMedium
	case "read_secret":
		return RiskCritical
	case "write_file":
		return RiskMedium
	case "shell":
		return RiskHigh
	case "network":
		return RiskMedium
	case "browser":
		return RiskHigh
	case "credential_use":
		return RiskHigh
	case "deploy":
		return RiskCritical
	case "destructive":
		return RiskCritical
	default:
		return RiskUnknown
	}
}
