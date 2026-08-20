package db

// RevokeToken persists a JWT ID to the revocation table so it cannot be used
// across process restarts.
func (db *DB) RevokeToken(jti string) error {
	_, err := db.conn.Exec(
		"INSERT OR IGNORE INTO jwt_revocations (jti) VALUES (?)",
		jti,
	)
	return err
}

// IsTokenRevoked checks whether a JWT ID has been persisted as revoked.
func (db *DB) IsTokenRevoked(jti string) (bool, error) {
	var count int
	err := db.conn.QueryRow(
		"SELECT COUNT(*) FROM jwt_revocations WHERE jti = ?",
		jti,
	).Scan(&count)
	if err != nil {
		return false, err
	}
	return count > 0, nil
}
