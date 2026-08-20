package memory

import (
	"context"
	"fmt"
	"log/slog"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/conflict"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/extractor"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
	"github.com/danieljustus/symaira-corekit/evidencekit"
	"github.com/google/uuid"
)

// Attribution carries provenance information about who created or updated a
// memory and in which session.
type Attribution struct {
	Author    string // Profile name, "cli:<user>" or JWT Subject
	SessionID string // e.g. Hermes session ID, empty if unknown
}

// Metadata Contract
//
// Every memory MUST carry these metadata keys in Memory.Metadata.
// The three layers are merged in order: provenance → trust → policy.
// Explicit values set by callers are never overwritten by defaults.
//
// Provenance (source tracking):
//   - source_type: "direct", "imported", or "extracted"
//   - source_tool: Client identifier ("cli", "mcp", "http", "import:<name>")
//   - source_uri: Origin URI when available (file path, session ID, URL)
//   - observed_at: When the source event occurred (RFC3339)
//   - extraction_method: For extracted facts ("pattern", "extractive_summary")
//   - evidence_snippet: Short quote from source that triggered the memory
//   - evidence_hash: SHA-256 of evidence_snippet for dedup
//
// Trust (retrieval filtering):
//   - authority: "direct", "inferred", "verified", "unverified"
//   - confidence: "high", "medium", "low" (direct=high, imported/inferred=medium)
//   - verification_status: "verified", "unverified", "stale"
//   - verified_at: When verification occurred (RFC3339)
//   - superseded_by: ID of the memory that replaced this one
//
// Policy (sensitivity and sharing):
//   - sensitivity: "public", "internal", "confidential", "secret"
//   - sharing_level: "private", "team", "org", "public"
//   - allowed_clients: CSV of client IDs allowed to read (empty = unrestricted)
//   - denied_clients: CSV of client IDs denied from reading
//
// Defaults applied by Prepare():
//   - Direct memories: authority=direct, confidence=high, sensitivity=internal, sharing_level=private
//   - Imported memories: authority=inferred, confidence=medium, sensitivity from importer, sharing_level=private
//   - Extracted memories: authority=inferred, confidence from parent, sensitivity/sharing from parent

// Standard provenance metadata keys stored in Memory.Metadata.
// Every memory MUST carry these fields; missing keys imply safe defaults.
//
// Required:
//   - source_type: How the memory was created ("direct", "imported", "extracted")
//   - source_tool: Which tool/client created it ("cli", "mcp", "http", "import:<name>")
//   - source_uri: Origin URI when available (file path, session ID, URL)
//   - observed_at: When the source event occurred (RFC3339)
//
// Optional:
//   - extraction_method: For extracted secondary facts ("pattern", "extractive_summary")
//   - evidence_snippet: Short quote from source that triggered the memory
//   - evidence_hash: SHA-256 of evidence_snippet for dedup
const (
	MetaSourceType       = "source_type"
	MetaSourceTool       = "source_tool"
	MetaSourceURI        = "source_uri"
	MetaObservedAt       = "observed_at"
	MetaExtractionMethod = "extraction_method"
	MetaEvidenceSnippet  = "evidence_snippet"
	MetaEvidenceHash     = "evidence_hash"
)

// Trust metadata keys for retrieval filtering.
// Missing keys imply safe defaults: authority="unverified", confidence="medium",
// verification_status="unverified".
const (
	MetaAuthority          = "authority"
	MetaConfidence         = "confidence"
	MetaVerificationStatus = "verification_status"
	MetaVerifiedAt         = "verified_at"
	MetaSupersededBy       = "superseded_by"
)

// Trust filter values.
const (
	AuthorityDirect     = "direct"
	AuthorityVerified   = "verified"
	AuthorityInferred   = "inferred"
	AuthorityUnverified = "unverified"

	ConfidenceHigh   = "high"
	ConfidenceMedium = "medium"
	ConfidenceLow    = "low"

	VerificationVerified   = "verified"
	VerificationUnverified = "unverified"
	VerificationStale      = "stale"
)

// Sensitivity and sharing policy metadata keys.
// Missing keys imply safe defaults: sensitivity="internal", sharing_level="private".
const (
	MetaSensitivity  = "sensitivity"
	MetaSharingLevel = "sharing_level"
)

// Sensitivity levels (ascending sensitivity).
const (
	SensitivityPublic       = "public"
	SensitivityInternal     = "internal"
	SensitivityConfidential = "confidential"
	SensitivitySecret       = "secret"
)

// Sharing levels (ascending visibility).
const (
	SharingPrivate = "private"
	SharingTeam    = "team"
	SharingOrg     = "org"
	SharingPublic  = "public"
)

// DefaultTrustMetadata returns standard trust metadata for a new memory.
// Direct memories are stamped with authority="direct" and confidence="high".
// Imported/extracted memories should use ImportedTrustMetadata() or
// ExtractionTrustMetadata() instead.
func DefaultTrustMetadata() map[string]string {
	return map[string]string{
		MetaAuthority:          AuthorityDirect,
		MetaConfidence:         ConfidenceHigh,
		MetaVerificationStatus: VerificationUnverified,
	}
}

// ImportedTrustMetadata returns trust metadata for an imported fact.
// Imported facts are inferred from external sources, so authority is "inferred"
// and confidence is "medium" until verified.
func ImportedTrustMetadata() map[string]string {
	return map[string]string{
		MetaAuthority:          AuthorityInferred,
		MetaConfidence:         ConfidenceMedium,
		MetaVerificationStatus: VerificationUnverified,
	}
}

// ExtractionTrustMetadata returns trust metadata for an extracted secondary fact.
// Extracted facts inherit confidence from the parent via MergeProvenance.
func ExtractionTrustMetadata(parentConfidence string) map[string]string {
	confidence := parentConfidence
	if confidence == "" {
		confidence = ConfidenceMedium
	}
	return map[string]string{
		MetaAuthority:          AuthorityInferred,
		MetaConfidence:         confidence,
		MetaVerificationStatus: VerificationUnverified,
	}
}

// DefaultPolicyMetadata returns standard sensitivity and sharing policy metadata.
// Memories default to sensitivity="internal" and sharing_level="private" so that
// new memories are conservative by default — they require explicit opt-in to
// share more broadly.
func DefaultPolicyMetadata() map[string]string {
	return map[string]string{
		MetaSensitivity:  SensitivityInternal,
		MetaSharingLevel: SharingPrivate,
	}
}

// DefaultProvenance returns the standard provenance metadata for a direct memory
// creation via CLI/MCP/HTTP. The sourceTool parameter identifies the client
// (e.g. "cli", "mcp", "http").
func DefaultProvenance(sourceTool string) map[string]string {
	now := time.Now().UTC().Format(time.RFC3339)
	return map[string]string{
		MetaSourceType: "direct",
		MetaSourceTool: sourceTool,
		MetaSourceURI:  "",
		MetaObservedAt: now,
	}
}

// ImportedProvenance returns provenance metadata for an imported fact.
func ImportedProvenance(sourceTool, sourceURI string, observedAt time.Time) map[string]string {
	ts := observedAt.Format(time.RFC3339)
	return map[string]string{
		MetaSourceType: "imported",
		MetaSourceTool: sourceTool,
		MetaSourceURI:  sourceURI,
		MetaObservedAt: ts,
	}
}

// ExtractionProvenance returns provenance metadata for an extracted secondary fact.
func ExtractionProvenance(method string) map[string]string {
	return map[string]string{
		MetaSourceType:       "extracted",
		MetaExtractionMethod: method,
	}
}

// MergeProvenance overlays provenance onto a base metadata map, preserving
// any pre-existing keys not in the provenance set.
func MergeProvenance(base, provenance map[string]string) map[string]string {
	if base == nil {
		base = make(map[string]string)
	}
	for k, v := range provenance {
		base[k] = v
	}
	return base
}

// Prepare validates scope, redacts PII, detects project boundaries,
// and returns a Memory ready for embedding generation and persistence.
// sourceTool identifies the client (e.g. "cli", "mcp", "http") and is used
// to stamp provenance metadata on the memory.
// The third return value is a RedactionResult summarizing any PII redactions
// that were applied, or nil if piiEnabled is false.
func Prepare(content, scope string, meta map[string]string, piiEnabled bool, attr Attribution, sourceTool string) (*db.Memory, *security.RedactionResult, error) {
	if scope == "" {
		scope = "global"
	}
	if err := security.ValidateScope(scope); err != nil {
		return nil, nil, err
	}

	var allMatches []security.RedactionMatch

	cleanContent := content
	if piiEnabled {
		result, rr := security.RedactWithResult(content)
		cleanContent = result
		if len(rr.Matches) > 0 {
			for _, m := range rr.Matches {
				slog.Warn("PII redacted from memory content", "pattern", m.PatternLabel)
			}
			allMatches = append(allMatches, rr.Matches...)
		}
	}

	if meta == nil {
		meta = make(map[string]string)
	}

	if scope == "project" {
		detector := security.NewProjectScopeDetector()
		meta["project_name"] = detector.DetectActiveProject()
	}

	provenance := DefaultProvenance(sourceTool)
	trust := DefaultTrustMetadata()
	policy := DefaultPolicyMetadata()

	defaults := make(map[string]string)
	defaults = MergeProvenance(defaults, provenance)
	defaults = MergeProvenance(defaults, trust)
	defaults = MergeProvenance(defaults, policy)

	meta = MergeProvenance(defaults, meta)

	cleanMeta := meta
	if piiEnabled {
		result, rr := security.RedactMapWithResult(meta)
		cleanMeta = result
		if len(rr.Matches) > 0 {
			for _, m := range rr.Matches {
				slog.Warn("PII redacted from memory metadata", "pattern", m.PatternLabel)
			}
			allMatches = append(allMatches, rr.Matches...)
		}
	}

	return &db.Memory{
		Content:        cleanContent,
		Scope:          scope,
		Metadata:       cleanMeta,
		CreatedBy:      attr.Author,
		UpdatedBy:      attr.Author,
		CreatedSession: attr.SessionID,
		UpdatedSession: attr.SessionID,
		ValidFrom:      ptrTime(time.Now().UTC()),
	}, &security.RedactionResult{Matches: allMatches}, nil
}

// StoreOptions tunes the write pipeline. The zero value reproduces the
// legacy behavior exactly.
type StoreOptions struct {
	// ConflictChecker enables write-path contradiction detection (#462).
	// nil (or a checker with Enabled=false) keeps the legacy behavior:
	// unconditional inserts, no dedup, no supersession.
	ConflictChecker *conflict.Checker
}

// Store wraps the full prepare → redact → embed → save → extract-facts pipeline.
// Returns the saved memory and any extracted secondary fact descriptions.
// The entities parameter contains entity names to link to the saved memory.
// sourceTool identifies the client (e.g. "cli", "mcp", "http").
func Store(database *db.DB, embeddings *extractor.EmbeddingsGenerator, patternExtractor *extractor.PatternExtractor, content, scope string, meta map[string]string, piiEnabled bool, attr Attribution, entities []string, sourceTool string, working bool, ttl time.Duration, opts ...StoreOptions) (*db.Memory, []string, error) {
	var checker *conflict.Checker
	if len(opts) > 0 {
		checker = opts[0].ConflictChecker
	}
	m, prepareResult, err := Prepare(content, scope, meta, piiEnabled, attr, sourceTool)
	if err != nil {
		return nil, nil, err
	}
	m.ID = uuid.New().String()

	if working {
		m.Tier = "working"
		expires := time.Now().UTC().Add(ttl)
		m.ExpiresAt = &expires
	} else {
		m.Tier = "long_term"
	}

	emb := embeddings.GenerateVector(m.Content)
	m.Embedding = emb.Vector
	m.EmbeddingSource = emb.Source
	m.EmbeddingModel = emb.Model
	m.EmbeddingQuantization = emb.Quantization

	// Write-path contradiction detection (#462): dedup repeats, resolve
	// contradictions, surface undecided pairs. Working memories (TTL
	// ephemera) and disabled checkers bypass the check entirely. A check
	// failure degrades to the legacy store-both behavior, never to a
	// failed write.
	if !working && checker != nil {
		stored, deduped, err := storeWithConflictCheck(database, checker, m, attr)
		if err != nil {
			return nil, nil, fmt.Errorf("failed to save memory: %w", err)
		}
		m = stored
		if deduped {
			// The fact already exists: keep the existing row, link any
			// entities onto it, and report it as the stored memory.
			slog.Info("conflict: duplicate write deduplicated", "id", m.ID)
		}
	} else if err := database.SaveMemory(m); err != nil {
		return nil, nil, fmt.Errorf("failed to save memory: %w", err)
	}

	for _, name := range entities {
		name = strings.TrimSpace(name)
		if name == "" {
			continue
		}
		entity, err := database.ResolveEntity(name)
		if err != nil {
			continue
		}
		if entity == nil {
			entity = &db.Entity{
				ID:        uuid.New().String(),
				Name:      name,
				Type:      "other",
				Aliases:   []string{},
				CreatedBy: attr.Author,
				CreatedAt: time.Now().UTC(),
			}
			if err := database.SaveEntity(entity); err != nil {
				continue
			}
		}
		_ = database.LinkMemoryToEntity(m.ID, entity.ID)
		m.Entities = append(m.Entities, entity.Name)
	}

	extractedFacts := patternExtractor.ExtractFacts(m.Content)
	var extractedStr []string

	// Collect unique redaction pattern labels for audit logging.
	allRedactionPatterns := make(map[string]struct{})
	if prepareResult != nil {
		for _, rm := range prepareResult.Matches {
			allRedactionPatterns[rm.PatternLabel] = struct{}{}
		}
	}

	for _, f := range extractedFacts {
		cleanFactContent := f.Content
		if piiEnabled {
			result, rr := security.RedactWithResult(f.Content)
			cleanFactContent = result
			if len(rr.Matches) > 0 {
				for _, m := range rr.Matches {
					slog.Warn("PII redacted from extracted fact content", "pattern", m.PatternLabel)
					allRedactionPatterns[m.PatternLabel] = struct{}{}
				}
			}
		}

		if isDuplicateOfPrimary(cleanFactContent, f.Metadata["raw_trigger"], m.Content) {
			continue
		}

		subID := uuid.New().String()
		subEmb := embeddings.GenerateVector(cleanFactContent)
		subVector := subEmb.Vector

		subMeta := f.Metadata
		if subMeta == nil {
			subMeta = make(map[string]string)
		}
		if m.Scope == "project" {
			subMeta["project_name"] = m.Metadata["project_name"]
		}

		extractionProv := ExtractionProvenance("pattern")
		subMeta = MergeProvenance(subMeta, extractionProv)

		parentConfidence := m.Metadata[MetaConfidence]
		extractionTrust := ExtractionTrustMetadata(parentConfidence)
		subMeta = MergeProvenance(subMeta, extractionTrust)

		if m.Metadata[MetaSensitivity] != "" {
			subMeta[MetaSensitivity] = m.Metadata[MetaSensitivity]
		}
		if m.Metadata[MetaSharingLevel] != "" {
			subMeta[MetaSharingLevel] = m.Metadata[MetaSharingLevel]
		}

		if piiEnabled {
			result, rr := security.RedactMapWithResult(subMeta)
			subMeta = result
			if len(rr.Matches) > 0 {
				for _, m := range rr.Matches {
					slog.Warn("PII redacted from extracted fact metadata", "pattern", m.PatternLabel)
					allRedactionPatterns[m.PatternLabel] = struct{}{}
				}
			}
		}

		subMem := &db.Memory{
			ID:              subID,
			Content:         cleanFactContent,
			Scope:           m.Scope,
			Metadata:        subMeta,
			Embedding:       subVector,
			EmbeddingSource: subEmb.Source,
			EmbeddingModel:  subEmb.Model,
			CreatedBy:       attr.Author,
			UpdatedBy:       attr.Author,
			CreatedSession:  attr.SessionID,
			UpdatedSession:  attr.SessionID,
		}
		if err := saveSubMemory(database, checker, subMem, attr); err == nil {
			extractedStr = append(extractedStr, fmt.Sprintf("  - [Fact Extracted] %s (ID: %s)", cleanFactContent, subID))

			if len(f.Evidence) > 0 {
				evidence := make([]evidencekit.Extraction, len(f.Evidence))
				for i, ext := range f.Evidence {
					ext.Source = evidencekit.SourceRef{ID: m.ID, Kind: sourceTool}
					if piiEnabled {
						result, rr := security.RedactWithResult(ext.EvidenceText)
						ext.EvidenceText = result
						if len(rr.Matches) > 0 {
							for _, m := range rr.Matches {
								slog.Warn("PII redacted from evidence text", "pattern", m.PatternLabel)
								allRedactionPatterns[m.PatternLabel] = struct{}{}
							}
						}
						result, rr = security.RedactWithResult(ext.Text)
						ext.Text = result
						if len(rr.Matches) > 0 {
							for _, m := range rr.Matches {
								slog.Warn("PII redacted from evidence text", "pattern", m.PatternLabel)
								allRedactionPatterns[m.PatternLabel] = struct{}{}
							}
						}
					}
					evidence[i] = ext
				}
				_ = database.SaveMemoryEvidence(subID, evidence)
			}
		}
	}

	// Write a single audit event aggregating all redaction pattern types.
	if len(allRedactionPatterns) > 0 {
		patterns := make([]string, 0, len(allRedactionPatterns))
		for p := range allRedactionPatterns {
			patterns = append(patterns, p)
		}
		if err := database.LogRedactionAudit(m.ID, m.Scope, attr.SessionID, attr.Author, patterns); err != nil {
			slog.Warn("failed to write redaction audit event", "error", err)
		}
	}

	return m, extractedStr, nil
}

// storeWithConflictCheck persists a memory through the write-path
// contradiction check (#462). It returns the stored memory (which is the
// existing row when the write was deduplicated) and whether the write was
// deduplicated. A repeat keeps the existing row and writes nothing; a
// contradiction marks the loser superseded and closes its valid_to; an
// undecided pair is stored unchanged and surfaced via a conflict_pending
// audit event. Check failures degrade to the legacy store-both behavior
// and never fail the write.
func storeWithConflictCheck(database *db.DB, checker *conflict.Checker, m *db.Memory, attr Attribution) (*db.Memory, bool, error) {
	res, err := checker.Check(context.Background(), m)
	if err != nil {
		slog.Warn("conflict check degraded; storing memory unchanged", "error", err)
		res = nil
	}
	if len(res) == 1 && res[0].Verdict == conflict.VerdictRepeat {
		return res[0].Candidate, true, nil
	}

	var contradictions []*conflict.Resolution
	for i := range res {
		r := &res[i]
		switch r.Verdict {
		case conflict.VerdictContradiction:
			contradictions = append(contradictions, r)
			if r.WinnerID != m.ID {
				// The existing fact wins: the new memory is saved with
				// the supersession pre-marked.
				m.SupersededBy = r.WinnerID
			}
		case conflict.VerdictAmbiguous:
			// Conservative default: store both and surface the
			// disagreement in the audit log.
			if err := database.LogAudit(conflict.ActionConflictPending, m.ID, m.Scope, attr.SessionID, attr.Author,
				conflict.ConflictPendingDetail(r.Candidate.ID, r.Candidate.CreatedBy, r.Similarity)); err != nil {
				slog.Warn("failed to write conflict_pending audit event", "error", err)
			}
		}
	}

	if err := database.SaveMemory(m); err != nil {
		return nil, false, err
	}

	for _, r := range contradictions {
		if r.WinnerID == m.ID {
			// New fact wins: mark the loser superseded and close its
			// validity window at the winner's creation time.
			if err := database.SupersedeMemory(r.LoserID, m.ID, m.CreatedAt, attr.Author, attr.SessionID); err != nil {
				slog.Warn("failed to mark memory superseded", "loser", r.LoserID, "error", err)
				continue
			}
		}
		// Record the resolution: both memory IDs, both actors, the
		// deciding rule — reviewable and reversible by hand.
		detail := conflict.SupersedeDetail(r.WinnerID, r.WinnerActor, r.LoserActor, r.Rule, r.Similarity)
		if err := database.LogAudit(conflict.ActionSupersede, r.LoserID, m.Scope, attr.SessionID, attr.Author, detail); err != nil {
			slog.Warn("failed to write supersede audit event", "error", err)
		}
	}
	return m, false, nil
}

// saveSubMemory persists a secondary extracted fact, running it through
// the same write-path conflict check as the primary when a checker is
// configured. Deduplicated sub-facts are skipped silently (the fact is
// already stored). Errors are returned so the caller can skip reporting
// the extraction, mirroring the legacy SaveMemory contract.
func saveSubMemory(database *db.DB, checker *conflict.Checker, subMem *db.Memory, attr Attribution) error {
	if checker == nil {
		return database.SaveMemory(subMem)
	}
	stored, deduped, err := storeWithConflictCheck(database, checker, subMem, attr)
	if err != nil {
		return err
	}
	if deduped {
		slog.Info("conflict: extracted fact deduplicated", "id", stored.ID)
		return nil
	}
	return nil
}

// FormatStoreSuccess builds a human-readable success message for a stored memory.
func FormatStoreSuccess(m *db.Memory, extractedStr []string) string {
	responseMsg := fmt.Sprintf("Successfully saved memory!\nMemory ID: %s\nContent: %s\nScope: %s", m.ID, m.Content, m.Scope)
	if m.Scope == "project" {
		responseMsg += fmt.Sprintf("\nProject: %s", m.Metadata["project_name"])
	}
	if len(extractedStr) > 0 {
		responseMsg += "\n\nAdditionally, secondary facts were successfully extracted:\n" + strings.Join(extractedStr, "\n")
	}
	return responseMsg
}

func ptrTime(t time.Time) *time.Time {
	return &t
}

func isDuplicateOfPrimary(fact, rawTrigger, primary string) bool {
	norm := func(s string) string {
		s = strings.ToLower(strings.TrimSpace(s))
		return strings.Trim(s, `.*"';!?`)
	}
	factNorm := norm(fact)
	primaryNorm := norm(primary)
	if factNorm == "" {
		return false
	}
	if factNorm == primaryNorm || strings.Contains(primaryNorm, factNorm) {
		return true
	}
	if rawTrigger != "" {
		return norm(rawTrigger) == primaryNorm
	}
	return false
}
