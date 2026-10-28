package syncclient

import (
	"context"
	"encoding/json"
	"fmt"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/security"
)

func TestPushTombstonePagesExcludeLiveUpserts(t *testing.T) {
	for _, relay := range []bool{false, true} {
		t.Run(map[bool]string{false: "plain", true: "relay"}[relay], func(t *testing.T) {
			t.Setenv("JWT_SECRET_KEY", "tombstone-test-secret")
			local := newTestPeer(t, "local")
			base := time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)
			for i := 0; i < pushBatchSize+1; i++ {
				id := fmt.Sprintf("deleted-%04d", i)
				storeMemory(t, local.db, id, "deleted content", base)
				if err := local.db.DeleteMemory(id); err != nil {
					t.Fatal(err)
				}
			}
			// This id has a delete history but is live when the continuation is read.
			storeMemory(t, local.db, "zz-live", "old content", base)
			if err := local.db.DeleteMemory("zz-live"); err != nil {
				t.Fatal(err)
			}
			storeMemory(t, local.db, "zz-live", "recreated content", base.Add(time.Second))
			// Leave delete timestamps exactly as SQLite's triggers wrote them.
			fake := &fakePushClient{failOnApplyNth: 4, failOnRelayNth: 4, applyErr: fmt.Errorf("pagination did not terminate"), relayErr: fmt.Errorf("pagination did not terminate")}
			opts := Options{DB: local.db, Passphrase: "phrase"}
			var err error
			if relay {
				_, err = relayPush(context.Background(), fake, opts, time.Time{}, &Result{})
			} else {
				_, err = plainPush(context.Background(), fake, opts, time.Time{}, &Result{})
			}
			if err != nil {
				t.Fatal(err)
			}
			seen := map[string]int{}
			live := 0
			if relay {
				if fake.relayPushCalls < 2 {
					t.Fatal("expected multiple relay pages")
				}
				engine := security.NewCryptoEngine()
				for _, batch := range fake.relayBlobBatches {
					for _, b := range batch {
						payload, err := engine.Decrypt(b.Blob, opts.Passphrase)
						if err != nil {
							t.Fatal(err)
						}
						var p relayPayload
						if err := json.Unmarshal(payload, &p); err != nil {
							t.Fatal(err)
						}
						if p.Deleted != nil {
							seen[p.Deleted.ID]++
						}
						if p.Memory != nil && p.Memory.ID == "zz-live" {
							live++
						}
					}
				}
			} else {
				if fake.applyCalls < 2 {
					t.Fatal("expected multiple plain pages")
				}
				for _, batch := range fake.applyDelBatches {
					for _, d := range batch {
						seen[d.ID]++
					}
				}
				for _, batch := range fake.applyMemBatches {
					for _, m := range batch {
						if m.ID == "zz-live" {
							live++
						}
					}
				}
			}
			if seen["zz-live"] != 0 {
				t.Errorf("live upsert emitted as delete %d times", seen["zz-live"])
			}
			if live != 1 {
				t.Errorf("live upserts=%d want 1", live)
			}
			for i := 0; i < pushBatchSize+1; i++ {
				id := fmt.Sprintf("deleted-%04d", i)
				if seen[id] != 1 {
					t.Errorf("%s sent %d times", id, seen[id])
				}
			}
		})
	}
}
