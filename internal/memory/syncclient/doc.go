// Package syncclient provides the in-process client for bidirectional
// remote memory synchronization.
//
// It speaks the same HTTP sync API the archived symmemory runtime exposed
// and consumed: GET/POST /api/sync/changes, /api/sync/apply and
// /api/sync/relay. Any process serving that API (typically another symbrain
// memory core over its embedded HTTP server, or a remote machine running the
// memory API over an SSH tunnel) can act as the remote peer — no shell-out,
// no separate binary.
//
// The local database is reused in place: sync cursors come from
// db.GetSyncCursor/SetSyncCursor per remote URL, local changes are read from
// the oplog (GetMemoriesSinceCursor + GetDeletedSince), and remote changes
// are applied with the tombstone-aware LWW helpers
// (SyncUpsertMemoryIfNewer/ApplyRemoteDelete). The conflict model is exactly
// the one the db package defines — this package never re-invents it.
//
// Two transports exist:
//
//   - Plain (default): pull via /api/sync/changes, push via /api/sync/apply.
//   - Encrypted relay: each change is JSON-marshaled and AES-256-GCM
//     encrypted (security.CryptoEngine) into an opaque blob stored via
//     /api/sync/relay, so the relay peer never sees plaintext memory
//     content. Tombstones travel as blobs under a reserved id prefix.
//
// Authentication is an optional bearer token sent as
// "Authorization: Bearer <token>" on every request.
package syncclient
