// Package approval defines the data contract for pending approval requests
// and human decisions in symguard's approval layer.
//
// The key design decision is embedded correlation: the ApprovalRequest
// carries the full original tool call (the ActionEvent from internal/model),
// so the frontend (TUI, CLI, or browser) never needs its own lookup state
// to know what it is approving. The ApprovalDecision echoes the same ID
// for correlation.
//
// This avoids a separate pending-request database or lookup table — the
// request IS the state, and matching on the echoed ID is sufficient for
// single-process, local-first operation.
//
// Standing permissions: a Decision with a non-zero TTL converts into a
// session-scoped grant (internal/grant) instead of an opaque timer, so the
// human can later enumerate and revoke what was allowed. Decisions without a
// TTL keep the original timer path unchanged.
package approval

import (
	"errors"
	"time"

	"github.com/danieljustus/symaira-guard/internal/grant"
	"github.com/danieljustus/symaira-guard/internal/model"
)

// Request is sent to the human for a decision on a pending tool call.
type Request struct {
	ID           string            `json:"id"`
	Hint         string            `json:"hint"`
	OriginalCall model.ActionEvent `json:"original_call"`
	Payload      any               `json:"payload,omitempty"`
	CreatedAt    time.Time         `json:"created_at"`
	TTL          time.Duration     `json:"ttl,omitempty"`
}

// Decision is the human's response to an approval request.
// The ID must echo the Request.ID for correlation.
type Decision struct {
	ID        string        `json:"id"`
	Approved  bool          `json:"approved"`
	Payload   any           `json:"payload,omitempty"`
	Reason    string        `json:"reason,omitempty"`
	TTL       time.Duration `json:"ttl,omitempty"`
	DecidedAt time.Time     `json:"decided_at"`
}

// Grant converts an approved decision with a non-zero TTL into a standing,
// session-scoped grant for subject. The grant's origin records the approval
// layer and the decision's epoch. Decisions without a TTL keep the opaque
// timer path: they return (nil, nil) and create no grant. A decision that is
// not approved never creates a grant either.
func (d Decision) Grant(subject string) (*grant.Grant, error) {
	if d.TTL <= 0 || !d.Approved {
		return nil, nil
	}
	if subject == "" {
		return nil, errors.New("approval: grant subject is empty")
	}
	decided := d.DecidedAt
	if decided.IsZero() {
		decided = time.Now()
	}
	return &grant.Grant{
		ID:        grant.NewID(),
		Scope:     grant.ScopeSession,
		Origin:    grant.Origin{Epoch: decided.Unix(), Via: "approval"},
		GrantedAt: decided,
		Subject:   subject,
	}, nil
}
