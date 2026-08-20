-- 030_audit_target.sql
-- Extend the audit log with a generic target so entity and relation events
-- are not shoehorned into memory_id. Memory events keep memory_id and leave
-- target_type/target_id NULL; entity and relation events set target_type
-- ("entity" | "relation") and target_id, leaving memory_id NULL.

ALTER TABLE audit_log ADD COLUMN target_type TEXT;
ALTER TABLE audit_log ADD COLUMN target_id TEXT;

CREATE INDEX IF NOT EXISTS idx_audit_log_target ON audit_log(target_type, target_id);
