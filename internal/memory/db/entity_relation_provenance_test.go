package db

import (
	"encoding/json"
	"errors"
	"testing"
)

func TestSaveEntityRelation_BackfillsID(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Bob")

	rel := &EntityRelation{FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "works-with"}
	if err := database.SaveEntityRelation(rel); err != nil {
		t.Fatalf("SaveEntityRelation: %v", err)
	}
	if rel.ID == "" {
		t.Fatal("expected SaveEntityRelation to assign a non-empty ID")
	}

	out, err := database.OutgoingRelations(ids["Alice"])
	if err != nil {
		t.Fatalf("OutgoingRelations: %v", err)
	}
	if len(out) != 1 || out[0].ID != rel.ID {
		t.Fatalf("expected persisted relation to carry the assigned ID, got %+v", out)
	}
}

func TestSaveEntityRelationProvenance_CreatesNewRelationWithProvenance(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Meeting")

	saved, err := database.SaveEntityRelationProvenance(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Meeting"], RelationType: "attended",
		Source: "symdesk", SourceRef: "meeting-123", Verification: VerificationVerified,
		Evidence: `{"source_doc_id":"doc-1","char_start":10,"char_end":20}`,
	})
	if err != nil {
		t.Fatalf("SaveEntityRelationProvenance: %v", err)
	}
	if saved.ID == "" {
		t.Fatal("expected a generated ID")
	}
	if saved.Source != "symdesk" || saved.SourceRef != "meeting-123" || saved.Verification != VerificationVerified {
		t.Fatalf("unexpected provenance: %+v", saved)
	}
	if saved.Evidence == "" {
		t.Fatal("expected evidence to be persisted")
	}

	fetched, err := database.GetEntityRelationByID(saved.ID)
	if err != nil {
		t.Fatalf("GetEntityRelationByID: %v", err)
	}
	if fetched == nil || fetched.SourceRef != "meeting-123" {
		t.Fatalf("expected relation retrievable by ID, got %+v", fetched)
	}
}

func TestSaveEntityRelationProvenance_RetryIsIdempotent(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Meeting")

	rel := &EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Meeting"], RelationType: "attended",
		Source: "symdesk", SourceRef: "meeting-123", Verification: VerificationUnverified,
	}
	first, err := database.SaveEntityRelationProvenance(rel)
	if err != nil {
		t.Fatalf("first save: %v", err)
	}

	second, err := database.SaveEntityRelationProvenance(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Meeting"], RelationType: "attended",
		Source: "symdesk", SourceRef: "meeting-123", Verification: VerificationUnverified,
	})
	if err != nil {
		t.Fatalf("retry save: %v", err)
	}
	if second.ID != first.ID {
		t.Fatalf("expected retry to return the same relation, got ID %s vs %s", second.ID, first.ID)
	}

	out, err := database.OutgoingRelations(ids["Alice"])
	if err != nil {
		t.Fatalf("OutgoingRelations: %v", err)
	}
	if len(out) != 1 {
		t.Fatalf("expected exactly 1 relation after idempotent retry, got %d", len(out))
	}
}

func TestSaveEntityRelationProvenance_VerifiedConflictIsRejected(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Meeting")

	if _, err := database.SaveEntityRelationProvenance(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Meeting"], RelationType: "attended",
		Source: "symdesk", SourceRef: "meeting-123", Verification: VerificationVerified,
	}); err != nil {
		t.Fatalf("initial verified save: %v", err)
	}

	_, err := database.SaveEntityRelationProvenance(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Meeting"], RelationType: "attended",
		Source: "other-tool", SourceRef: "different-ref", Verification: VerificationUnverified,
	})
	if err == nil {
		t.Fatal("expected a conflict error when overwriting a verified relation with different provenance")
	}
	var conflictErr *VerifiedRelationConflictError
	if !errors.As(err, &conflictErr) {
		t.Fatalf("expected *VerifiedRelationConflictError, got %T: %v", err, err)
	}

	// The original verified provenance must survive untouched.
	existing, err := database.getRelationByTriple(ids["Alice"], ids["Meeting"], "attended")
	if err != nil {
		t.Fatalf("getRelationByTriple: %v", err)
	}
	if existing.Source != "symdesk" || existing.SourceRef != "meeting-123" {
		t.Fatalf("expected verified provenance to survive unchanged, got %+v", existing)
	}
}

func TestSaveEntityRelationProvenance_EnrichesUnverifiedLegacyRelation(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Meeting")

	// Legacy bare relation, no provenance.
	if err := database.SaveEntityRelation(&EntityRelation{FromEntityID: ids["Alice"], ToEntityID: ids["Meeting"], RelationType: "attended"}); err != nil {
		t.Fatalf("legacy save: %v", err)
	}

	updated, err := database.SaveEntityRelationProvenance(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Meeting"], RelationType: "attended",
		Source: "symdesk", SourceRef: "meeting-123", Verification: VerificationVerified,
	})
	if err != nil {
		t.Fatalf("enrich save: %v", err)
	}
	if updated.Source != "symdesk" || updated.Verification != VerificationVerified {
		t.Fatalf("expected legacy relation to be enriched with provenance, got %+v", updated)
	}

	out, err := database.OutgoingRelations(ids["Alice"])
	if err != nil {
		t.Fatalf("OutgoingRelations: %v", err)
	}
	if len(out) != 1 {
		t.Fatalf("expected the legacy row to be enriched in place, not duplicated, got %d rows", len(out))
	}
}

func TestSaveEntityRelationProvenance_IDSurvivesEntityRename(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Meeting")

	saved, err := database.SaveEntityRelationProvenance(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Meeting"], RelationType: "attended",
		Source: "symdesk", SourceRef: "meeting-123", Verification: VerificationVerified,
	})
	if err != nil {
		t.Fatalf("save: %v", err)
	}

	renamed, err := database.GetEntityByID(ids["Alice"])
	if err != nil {
		t.Fatalf("GetEntityByID: %v", err)
	}
	renamed.Name = "Alicia"
	if err := database.SaveEntity(renamed); err != nil {
		t.Fatalf("rename entity: %v", err)
	}

	fetched, err := database.GetEntityRelationByID(saved.ID)
	if err != nil {
		t.Fatalf("GetEntityRelationByID after rename: %v", err)
	}
	if fetched == nil || fetched.FromEntityID != ids["Alice"] {
		t.Fatalf("expected relation to survive entity rename by ID, got %+v", fetched)
	}
}

func TestSaveEntityRelationProvenance_InvalidVerificationRejected(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Meeting")

	_, err := database.SaveEntityRelationProvenance(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Meeting"], RelationType: "attended",
		Verification: "definitely-real",
	})
	if err == nil {
		t.Fatal("expected an error for an invalid verification value")
	}
}

func TestValidateRelationEvidence_ValidSpans(t *testing.T) {
	tests := []struct {
		name string
		in   string
	}{
		{"empty is valid", ""},
		{"whitespace only is valid", "   "},
		{"source doc id only", `{"source_doc_id":"doc-1"}`},
		{"char span", `{"char_start":0,"char_end":100}`},
		{"time span", `{"time_start":1.5,"time_end":30.0}`},
		{"full shape", `{"source_doc_id":"doc-1","char_start":0,"char_end":100,"time_start":0,"time_end":10}`},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if _, err := ValidateRelationEvidence(tt.in); err != nil {
				t.Errorf("ValidateRelationEvidence(%q) unexpected error: %v", tt.in, err)
			}
		})
	}
}

func TestValidateRelationEvidence_RejectsInvalid(t *testing.T) {
	tests := []struct {
		name string
		in   string
	}{
		{"malformed JSON", `{not json`},
		{"char_end before char_start", `{"char_start":50,"char_end":10}`},
		{"time_end before time_start", `{"time_start":10,"time_end":1}`},
		{"negative char_start", `{"char_start":-1,"char_end":10}`},
		{"negative time_start", `{"time_start":-5,"time_end":10}`},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if _, err := ValidateRelationEvidence(tt.in); err == nil {
				t.Errorf("ValidateRelationEvidence(%q) expected an error, got none", tt.in)
			}
		})
	}
}

func TestValidateRelationEvidence_RejectsOversized(t *testing.T) {
	huge := make([]byte, MaxEvidenceBytes+1)
	for i := range huge {
		huge[i] = 'a'
	}
	if _, err := ValidateRelationEvidence(`{"source_doc_id":"` + string(huge) + `"}`); err == nil {
		t.Fatal("expected an error for oversized evidence")
	}
}

func TestValidateRelationEvidence_DropsUnknownFields(t *testing.T) {
	canon, err := ValidateRelationEvidence(`{"source_doc_id":"doc-1","unexpected_huge_field":"should be dropped"}`)
	if err != nil {
		t.Fatalf("ValidateRelationEvidence: %v", err)
	}
	if canon == "" {
		t.Fatal("expected canonicalized evidence")
	}
	var ev RelationEvidence
	if err := json.Unmarshal([]byte(canon), &ev); err != nil {
		t.Fatalf("unmarshal canonicalized evidence: %v", err)
	}
	if ev.SourceDocID != "doc-1" {
		t.Fatalf("expected source_doc_id preserved, got %+v", ev)
	}
}

func TestSaveEntityRelationProvenance_RejectsInvalidEvidenceBeforeMutation(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Meeting")

	_, err := database.SaveEntityRelationProvenance(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Meeting"], RelationType: "attended",
		Source: "symdesk", SourceRef: "meeting-123", Evidence: `{not valid json`,
	})
	if err == nil {
		t.Fatal("expected an error for invalid evidence")
	}

	out, err := database.OutgoingRelations(ids["Alice"])
	if err != nil {
		t.Fatalf("OutgoingRelations: %v", err)
	}
	if len(out) != 0 {
		t.Fatalf("expected no relation to be persisted after evidence rejection, got %d", len(out))
	}
}

func TestGraphNeighbors_IncludesProvenanceFields(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Meeting")

	if _, err := database.SaveEntityRelationProvenance(&EntityRelation{
		FromEntityID: ids["Alice"], ToEntityID: ids["Meeting"], RelationType: "attended",
		Source: "symdesk", SourceRef: "meeting-123", Verification: VerificationVerified,
	}); err != nil {
		t.Fatalf("save: %v", err)
	}

	_, edges, err := database.GraphNeighbors(ids["Alice"], 1)
	if err != nil {
		t.Fatalf("GraphNeighbors: %v", err)
	}
	if len(edges) != 1 {
		t.Fatalf("expected 1 edge, got %d", len(edges))
	}
	if edges[0].Source != "symdesk" || edges[0].Verification != VerificationVerified || edges[0].ID == "" {
		t.Fatalf("expected provenance fields on graph edge, got %+v", edges[0])
	}
}

func TestDeleteEntityRelationByID(t *testing.T) {
	database := newTestDB(t)
	ids := seedRelationEntities(t, database, "Alice", "Bob")

	rel := &EntityRelation{FromEntityID: ids["Alice"], ToEntityID: ids["Bob"], RelationType: "works-with"}
	if err := database.SaveEntityRelation(rel); err != nil {
		t.Fatalf("save: %v", err)
	}

	if err := database.DeleteEntityRelationByID(rel.ID); err != nil {
		t.Fatalf("DeleteEntityRelationByID: %v", err)
	}

	fetched, err := database.GetEntityRelationByID(rel.ID)
	if err != nil {
		t.Fatalf("GetEntityRelationByID: %v", err)
	}
	if fetched != nil {
		t.Fatalf("expected relation to be deleted, got %+v", fetched)
	}
}
