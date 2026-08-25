package syncclient

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
)

// tombstoneBlobPrefix marks relay blobs that carry a delete tombstone rather
// than a memory; the relay blind-stores both, the recipient distinguishes
// them after decryption by this id prefix.
const tombstoneBlobPrefix = "tombstone:"

// Options configures one sync run against a single remote.
type Options struct {
	// Remote is the base URL of the remote memory server (required).
	Remote string
	// Token is an optional bearer token for the remote API.
	Token string
	// Pull fetches and applies remote changes; Push sends local changes.
	// Both false is rejected; the CLI defaults them to both.
	Pull bool
	Push bool
	// EncryptedRelay exchanges AES-256-GCM encrypted blobs through
	// /api/sync/relay instead of plain changes/apply.
	EncryptedRelay bool
	// Passphrase is required when EncryptedRelay is set. Never log it.
	Passphrase string
	// DB is the local memory database whose sync state is reused (required).
	DB *db.DB
	// PageLimit bounds each pull page; 0 = defaultPageLimit.
	PageLimit int
	// Timeout bounds the whole run; 0 = defaultTimeout.
	Timeout time.Duration
}

// Result summarizes one sync run. Counters describe locally observable
// effects: pulled_* count changes applied on this side (last-writer-wins),
// pushed_* mirror the remote's apply/skip counters, relay_* count blobs.
type Result struct {
	Remote         string    `json:"remote"`
	Mode           string    `json:"mode"` // "pull", "push" or "both"
	EncryptedRelay bool      `json:"encrypted_relay"`
	Cursor         time.Time `json:"cursor"` // sync cursor stored for this remote
	ServerTime     time.Time `json:"server_time"`

	PulledMemories int `json:"pulled_memories_applied"`
	PulledDeletes  int `json:"pulled_deletes_applied"`
	PushedMemories int `json:"pushed_memories"`
	PushedDeletes  int `json:"pushed_deletes"`
	RelayFetched   int `json:"relay_blobs_fetched"`
	RelayStored    int `json:"relay_blobs_stored"`
}

// Run executes one sync against the remote: pull remote changes since the
// stored cursor, push local changes since the same cursor, then store the
// combined cursor (max of the remote's server time and the newest local
// change sent) via SetSyncCursor. All changes are applied with the db
// package's tombstone-aware LWW helpers, so the conflict model is unchanged.
func Run(ctx context.Context, opts Options) (*Result, error) {
	if opts.Remote == "" {
		return nil, errors.New("remote URL is required")
	}
	if opts.DB == nil {
		return nil, errors.New("database is required")
	}
	if !opts.Pull && !opts.Push {
		return nil, errors.New("at least one of pull or push must be enabled")
	}
	if opts.EncryptedRelay && opts.Passphrase == "" {
		return nil, errors.New("encrypted relay requires a passphrase")
	}
	timeout := opts.Timeout
	if timeout <= 0 {
		timeout = defaultTimeout
	}
	if ctx == nil {
		ctx = context.Background()
	}
	ctx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()

	cursor, err := opts.DB.GetSyncCursor(opts.Remote)
	if err != nil {
		return nil, fmt.Errorf("read sync cursor for %s: %w", opts.Remote, err)
	}

	res := &Result{Remote: opts.Remote, EncryptedRelay: opts.EncryptedRelay}
	switch {
	case opts.Pull && opts.Push:
		res.Mode = "both"
	case opts.Pull:
		res.Mode = "pull"
	default:
		res.Mode = "push"
	}

	client := NewClient(opts.Remote, opts.Token, nil)
	pageLimit := opts.PageLimit
	if pageLimit <= 0 {
		pageLimit = defaultPageLimit
	}

	var serverTime, localMax time.Time
	if opts.EncryptedRelay {
		serverTime, err = relayPull(ctx, client, opts, cursor, pageLimit, res)
		if err != nil {
			return nil, fmt.Errorf("relay pull from %s: %w", opts.Remote, err)
		}
		if opts.Push {
			localMax, err = relayPush(ctx, client, opts, cursor, res)
			if err != nil {
				return nil, fmt.Errorf("relay push to %s: %w", opts.Remote, err)
			}
		}
	} else {
		if opts.Pull {
			serverTime, err = plainPull(ctx, client, opts, cursor, pageLimit, res)
			if err != nil {
				return nil, fmt.Errorf("pull from %s: %w", opts.Remote, err)
			}
		}
		if opts.Push {
			localMax, err = plainPush(ctx, client, opts, cursor, res)
			if err != nil {
				return nil, fmt.Errorf("push to %s: %w", opts.Remote, err)
			}
		}
	}

	// The cursor is the max of what the remote reported as its clock and the
	// newest local change we sent, so neither side re-receives its own data
	// on the next run.
	res.ServerTime = serverTime
	newCursor := maxTime(serverTime, localMax)
	if err := opts.DB.SetSyncCursor(opts.Remote, newCursor); err != nil {
		return nil, fmt.Errorf("persist sync cursor for %s: %w", opts.Remote, err)
	}
	res.Cursor = newCursor
	return res, nil
}

// plainPull applies remote memories (then tombstones, matching the server's
// own apply order) and returns the newest timestamp observed on the remote
// side, which becomes the lower bound for the stored cursor.
func plainPull(ctx context.Context, client *Client, opts Options, since time.Time, limit int, res *Result) (time.Time, error) {
	var maxSeen time.Time
	cursor := ""
	for {
		changes, err := client.Changes(ctx, since, cursor, limit)
		if err != nil {
			return maxSeen, err
		}
		for _, m := range changes.Memories {
			applied, err := opts.DB.SyncUpsertMemoryIfNewer(m)
			if err != nil {
				return maxSeen, fmt.Errorf("apply remote memory %s: %w", m.ID, err)
			}
			if applied {
				res.PulledMemories++
			}
			maxSeen = maxTime(maxSeen, m.UpdatedAt)
		}
		for _, d := range changes.Deleted {
			removed, err := opts.DB.ApplyRemoteDelete(d.ID, d.DeletedAt)
			if err != nil {
				return maxSeen, fmt.Errorf("apply remote delete %s: %w", d.ID, err)
			}
			if removed {
				res.PulledDeletes++
			}
			maxSeen = maxTime(maxSeen, d.DeletedAt)
		}
		maxSeen = maxTime(maxSeen, changes.ServerTime)
		if changes.NextCursor == "" {
			return maxSeen, nil
		}
		cursor = changes.NextCursor
		since = time.Time{} // the pagination cursor takes precedence
	}
}

// plainPush sends local changes since the cursor and returns the newest
// local change timestamp sent.
func plainPush(ctx context.Context, client *Client, opts Options, since time.Time, res *Result) (time.Time, error) {
	memories, err := opts.DB.GetMemoriesSinceCursor(since, 0)
	if err != nil {
		return time.Time{}, fmt.Errorf("read local changes: %w", err)
	}
	deleted, err := opts.DB.GetDeletedSince(since)
	if err != nil {
		return time.Time{}, fmt.Errorf("read local tombstones: %w", err)
	}
	if len(memories) == 0 && len(deleted) == 0 {
		return time.Time{}, nil
	}
	applied, err := client.Apply(ctx, memories, deleted)
	if err != nil {
		return time.Time{}, err
	}
	res.PushedMemories = applied.Applied
	res.PushedDeletes = applied.Deleted
	return maxLocalChangeTs(memories, deleted), nil
}

// relayPayload is the plaintext carried inside an encrypted relay blob.
// Exactly one of Memory/Deleted is set.
type relayPayload struct {
	Memory  *db.Memory        `json:"memory,omitempty"`
	Deleted *db.DeletedMemory `json:"deleted,omitempty"`
}

// relayPull fetches encrypted blobs since the cursor, decrypts and applies
// them in timestamp order, and returns the newest timestamp observed (the
// relay's server clock or the newest blob timestamp from a peer).
func relayPull(ctx context.Context, client *Client, opts Options, since time.Time, limit int, res *Result) (time.Time, error) {
	engine := security.NewCryptoEngine()
	var maxSeen time.Time
	pageSince := since
	for {
		resp, err := client.RelayPull(ctx, pageSince, limit)
		if err != nil {
			return maxSeen, err
		}
		maxSeen = maxTime(maxSeen, resp.ServerTime)
		for _, b := range resp.Blobs {
			applied, err := applyRelayBlob(engine, opts, b, res)
			if err != nil {
				return maxSeen, err
			}
			if applied {
				res.RelayFetched++
			}
			maxSeen = maxTime(maxSeen, b.UpdatedAt)
		}
		if len(resp.Blobs) < limit {
			return maxSeen, nil
		}
		// Blobs arrive ordered by updated_at ascending; the relay returns
		// strictly after the requested since, so the last timestamp
		// guarantees forward progress on the next page.
		pageSince = resp.Blobs[len(resp.Blobs)-1].UpdatedAt
	}
}

// applyRelayBlob decrypts one relay blob and applies it to the local
// database. It reports whether the blob was recognized (not skipped).
func applyRelayBlob(engine *security.CryptoEngine, opts Options, b db.RelayBlob, res *Result) (bool, error) {
	payload, err := engine.Decrypt(b.Blob, opts.Passphrase)
	if err != nil {
		return false, fmt.Errorf("decrypt relay blob %s: %w", b.ID, err)
	}
	var p relayPayload
	if err := json.Unmarshal(payload, &p); err != nil {
		return false, fmt.Errorf("decode relay blob %s: %w", b.ID, err)
	}
	switch {
	case p.Deleted != nil:
		removed, err := opts.DB.ApplyRemoteDelete(p.Deleted.ID, p.Deleted.DeletedAt)
		if err != nil {
			return false, fmt.Errorf("apply relay delete %s: %w", p.Deleted.ID, err)
		}
		if removed {
			res.PulledDeletes++
		}
		return true, nil
	case p.Memory != nil:
		applied, err := opts.DB.SyncUpsertMemoryIfNewer(p.Memory)
		if err != nil {
			return false, fmt.Errorf("apply relay memory %s: %w", p.Memory.ID, err)
		}
		if applied {
			res.PulledMemories++
		}
		return true, nil
	default:
		return false, nil // empty payload blob: ignore
	}
}

// relayPush encrypts local changes into blobs and stores them on the remote
// relay, returning the newest local change timestamp sent.
func relayPush(ctx context.Context, client *Client, opts Options, since time.Time, res *Result) (time.Time, error) {
	memories, err := opts.DB.GetMemoriesSinceCursor(since, 0)
	if err != nil {
		return time.Time{}, fmt.Errorf("read local changes: %w", err)
	}
	deleted, err := opts.DB.GetDeletedSince(since)
	if err != nil {
		return time.Time{}, fmt.Errorf("read local tombstones: %w", err)
	}
	if len(memories) == 0 && len(deleted) == 0 {
		return time.Time{}, nil
	}

	engine := security.NewCryptoEngine()
	blobs := make([]db.RelayBlob, 0, len(memories)+len(deleted))
	for _, m := range memories {
		blob, err := encryptRelay(engine, relayPayload{Memory: m}, opts.Passphrase)
		if err != nil {
			return time.Time{}, err
		}
		blobs = append(blobs, db.RelayBlob{ID: m.ID, UpdatedAt: m.UpdatedAt, Blob: blob})
	}
	for _, d := range deleted {
		blob, err := encryptRelay(engine, relayPayload{Deleted: &d}, opts.Passphrase)
		if err != nil {
			return time.Time{}, err
		}
		blobs = append(blobs, db.RelayBlob{ID: tombstoneBlobPrefix + d.ID, UpdatedAt: d.DeletedAt, Blob: blob})
	}

	got, err := client.RelayPush(ctx, blobs)
	if err != nil {
		return time.Time{}, err
	}
	res.RelayStored = got.Stored
	return maxLocalChangeTs(memories, deleted), nil
}

func encryptRelay(engine *security.CryptoEngine, p relayPayload, passphrase string) ([]byte, error) {
	payload, err := json.Marshal(p)
	if err != nil {
		return nil, fmt.Errorf("encode relay payload: %w", err)
	}
	blob, err := engine.Encrypt(payload, passphrase)
	if err != nil {
		return nil, fmt.Errorf("encrypt relay payload: %w", err)
	}
	return blob, nil
}

// maxLocalChangeTs returns the newest timestamp among the local changes
// that were sent; zero time when nothing was sent.
func maxLocalChangeTs(memories []*db.Memory, deleted []db.DeletedMemory) time.Time {
	var max time.Time
	for _, m := range memories {
		max = maxTime(max, m.UpdatedAt)
	}
	for _, d := range deleted {
		max = maxTime(max, d.DeletedAt)
	}
	return max
}

func maxTime(a, b time.Time) time.Time {
	if a.After(b) {
		return a
	}
	return b
}
