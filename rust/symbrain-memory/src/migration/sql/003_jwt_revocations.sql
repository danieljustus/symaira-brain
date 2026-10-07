CREATE TABLE IF NOT EXISTS jwt_revocations (
    jti TEXT PRIMARY KEY,
    revoked_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_jwt_revocations_jti ON jwt_revocations(jti);
