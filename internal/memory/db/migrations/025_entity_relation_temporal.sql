-- Temporal validity for entity relations: valid_from and valid_until columns
-- support bi-temporal queries ("what relation held on date X?").
-- Existing rows remain open-ended (both NULL = always valid).
-- A version chain closes the previous open interval when a newer version of
-- the same (from, to, relation_type) triple is created with a valid_from.

ALTER TABLE entity_relations ADD COLUMN valid_from DATETIME;
ALTER TABLE entity_relations ADD COLUMN valid_until DATETIME;

CREATE INDEX idx_entity_relations_valid_from ON entity_relations(valid_from);
CREATE INDEX idx_entity_relations_valid_until ON entity_relations(valid_until);
