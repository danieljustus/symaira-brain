package usage

import "time"

// ReportSchemaVersion identifies the machine-readable usage report contract.
// Consumers should reject unknown major schema versions.
const ReportSchemaVersion = 1

// Report is the stable result returned by "symbrain usage --output json".
type Report struct {
	SchemaVersion int             `json:"schema_version"`
	Providers     []ProviderUsage `json:"providers"`
}

// ProviderUsage describes one AI provider's configuration and usage.
// Configured/AuthStatus are always present so a caller can render "not set
// up" without attempting a fetch; Snapshot/Error are populated only after an
// actual fetch attempt (which "usage" always performs for configured
// providers, but the CLI/MCP caller decides whether to fetch unconfigured
// ones at all — typically not, since the result would just be "missing").
//
// This mirrors symaira-cockpit's AIUsageProvider protocol
// (tune/Sources/SymTuneCore/AIUsage.swift:121): id, displayName, isConfigured
// map directly; credentialDescriptor/credentialSource collapse into
// AuthStatus (see its doc comment).
type ProviderUsage struct {
	ID          string     `json:"id"`
	DisplayName string     `json:"display_name"`
	Configured  bool       `json:"configured"`
	AuthStatus  AuthStatus `json:"auth_status"`
	// Snapshot is nil when the provider is not configured or the fetch
	// failed; Error then carries the reason.
	Snapshot *UsageSnapshot `json:"snapshot,omitempty"`
	Error    string         `json:"error,omitempty"`
}

// AuthStatus reports how a provider's credential resolved, for a consumer
// that wants to show "signed in via Claude Code OAuth" or "re-auth needed"
// without itself understanding any provider's auth flow.
//
// Mirrors symaira-cockpit's ExternalAuthState (AIUsage.swift:181-204).
// Status values match ExternalAuthState.Status verbatim (available, missing,
// expired, partial) so a Swift consumer can decode this directly into that
// enum without a translation table.
type AuthStatus struct {
	// Status is one of: available, missing, expired, partial.
	Status string `json:"status"`
	// Detail is a human-readable description, e.g. "via Claude Code OAuth
	// token" or "re-auth needed".
	Detail string `json:"detail"`
	// Source is how the credential was resolved (oauth, cli, file,
	// keyring, symvault, env) — empty when Status is "missing".
	Source string `json:"source,omitempty"`
}

// UsageSnapshot is one provider's normalized usage read, produced by
// whichever fallback strategy succeeded first.
//
// Mirrors symaira-cockpit's AIUsageSnapshot (AIUsage.swift:53-108).
// ProviderID is repeated here (not just on the parent ProviderUsage) so a
// UsageSnapshot stays self-describing if ever extracted on its own — the
// Swift original has the same redundancy for the same reason.
type UsageSnapshot struct {
	ProviderID string       `json:"provider_id"`
	Meters     []UsageMeter `json:"meters"`
	// Balance and Currency are the remaining amount for balance-style
	// providers (e.g. credits, spend). Nil when the provider has no
	// single balance figure (meter-only providers).
	Balance   *string   `json:"balance,omitempty"`
	Currency  *string   `json:"currency,omitempty"`
	FetchedAt time.Time `json:"fetched_at"`
	// Source is which fallback strategy produced this snapshot (oauth,
	// cli, web, api, local) — kept visible so a brittle active path
	// stays visible in the UI/CLI, exactly as in the Swift original.
	Source string `json:"source"`
}

// UsageMeter is one normalized usage meter (e.g. "this session", "this
// week", "credits"). Mirrors symaira-cockpit's AIUsageMeter
// (AIUsage.swift:27-48).
type UsageMeter struct {
	Label string `json:"label"`
	// Used and Limit are decimal amounts serialized as strings (not JSON
	// numbers) to avoid float64 precision loss on token/credit counts —
	// Swift's original type is Decimal, which Go's encoding/json has no
	// equivalent for. Nil when the provider does not report that side of
	// the meter (e.g. limit unknown).
	Used  *string `json:"used,omitempty"`
	Limit *string `json:"limit,omitempty"`
	// Unit is the pre-formatted display label: "tokens", "requests",
	// "credits", "%", or a currency code (e.g. "USD"). This flattens
	// Swift's AIUsageUnit enum (tokens/requests/credits/currency(code)/
	// percent) to its already-computed unitLabel string — a consumer
	// never needs to branch on unit kind, only display it.
	Unit string `json:"unit"`
	// ResetsAt is when this meter resets, if known.
	ResetsAt *time.Time `json:"resets_at,omitempty"`
}
