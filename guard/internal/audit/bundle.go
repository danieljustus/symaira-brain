// Package audit defines the types for symguard's append-only audit log,
// evidence references, and portable security case bundles.
//
// Evidence references point to supporting data without copying it, using
// ref:// URIs. Case bundles package events, decisions, and audit records
// into a portable, redaction-safe directory structure.
package audit

// BundleSchemaVersion is the current version of the case bundle format.
const BundleSchemaVersion = 1

// EvidenceRef is a URI-style reference to supporting evidence without
// copying the raw data. Formats:
//
//	ref://file/path:line         local file and line number
//	ref://event/<event-id>       reference to another event in the case
//	ref://hash/<sha256>          content-addressed reference
type EvidenceRef string

// Manifest describes a portable security case bundle.
type Manifest struct {
	SchemaVersion int               `json:"schema_version"`
	CaseID        string            `json:"case_id"`
	CreatedAt     string            `json:"created_at"` // RFC 3339
	RecordCounts  map[string]int    `json:"record_counts"`
	Digests       map[string]string `json:"digests"` // SHA-256 per stream
	SourceRepo    string            `json:"source_repo,omitempty"`
	Unsigned      bool              `json:"unsigned"`
}

// CaseBundle is a portable collection of security evidence.
type CaseBundle struct {
	Manifest  Manifest   `json:"manifest"`
	Events    []Event    `json:"events,omitempty"`
	Decisions []Decision `json:"decisions,omitempty"`
}

// Event is a normalized audit event record.
type Event struct {
	ID         string        `json:"id"`
	SchemaVer  int           `json:"schema_version"`
	EventType  string        `json:"event_type"`
	AgentID    string        `json:"agent_id,omitempty"`
	Server     string        `json:"server,omitempty"`
	Tool       string        `json:"tool,omitempty"`
	Capability string        `json:"capability,omitempty"`
	Decision   string        `json:"decision"`
	Evidence   []EvidenceRef `json:"evidence,omitempty"`
	Redacted   bool          `json:"redacted"`
	Timestamp  string        `json:"timestamp"` // RFC 3339
}

// Decision is a portable approval decision record.
type Decision struct {
	ID        string `json:"id"`
	RequestID string `json:"request_id"`
	Approved  bool   `json:"approved"`
	Reason    string `json:"reason,omitempty"`
	DecidedAt string `json:"decided_at"` // RFC 3339
}

// ProposalApplied is a portable audit record for an applied policy
// proposal. It is the proposal counterpart of Decision: it identifies
// the resulting rule and the human who applied it, so a rule's
// provenance can be traced through the audit chain.
type ProposalApplied struct {
	ProposalID string `json:"proposal_id"`
	Action     string `json:"action"`     // "set" or "delete"
	Rule       string `json:"rule"`       // canonical JSON of the resulting rule
	AppliedBy  string `json:"applied_by"` // human who applied the proposal
	AppliedAt  string `json:"applied_at"` // RFC 3339
}

// ExternalDecision is the audit record for one decision produced by the
// external classifier interface (`symguard decide`, issue #81): the
// request fields that were evaluated, the resulting decision
// (allow|confirm|deny), and the reason. A concrete hash-chained sink for
// records like this is Phase 3 wiring; the emitting package consumes the
// record through its own small Sink interface.
type ExternalDecision struct {
	ID        string   `json:"id"`
	Command   string   `json:"command"`
	RiskClass string   `json:"risk_class,omitempty"`
	Domain    string   `json:"domain,omitempty"`
	Warnings  []string `json:"warnings,omitempty"`
	Decision  string   `json:"decision"` // allow|confirm|deny
	Reason    string   `json:"reason,omitempty"`
	DecidedAt string   `json:"decided_at"` // RFC 3339
}

// NewManifest creates a manifest with default values for the current schema.
func NewManifest(caseID string) Manifest {
	return Manifest{
		SchemaVersion: BundleSchemaVersion,
		CaseID:        caseID,
		RecordCounts:  make(map[string]int),
		Digests:       make(map[string]string),
		Unsigned:      true,
	}
}
