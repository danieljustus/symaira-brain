// PREPARED private-copy caller. Seeds through actual frozen public owners and
// invokes real retention; no production implementation is copied or changed.
package main

import (
	"encoding/json"
	"os"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/activity"
	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

type packet struct {
	ID, Operation, Started, Ended, Expiry, At, Start, End string
}

func instant(raw string) time.Time {
	t, err := time.Parse(time.RFC3339Nano, raw)
	if err != nil {
		panic(err)
	}
	return t
}

func main() {
	if len(os.Args) != 4 {
		panic("usage: owned-retention761 seed|apply INPUT.json DATABASE")
	}
	data, err := os.ReadFile(os.Args[2])
	if err != nil {
		panic(err)
	}
	var p packet
	if err = json.Unmarshal(data, &p); err != nil {
		panic(err)
	}
	cfg := config.Defaults()
	cfg.Database.Path = os.Args[3]
	database, err := db.Open(cfg)
	if err != nil {
		panic(err)
	}
	defer func() {
		if err := database.Close(); err != nil {
			panic(err)
		}
	}()
	owner, err := activity.NewStore(database)
	if err != nil {
		panic(err)
	}
	if os.Args[1] == "seed" {
		err = owner.SaveSegment(activity.Segment{ID: "owned", Source: "owned", Granularity: "10min",
			StartedAt: instant(p.Started), EndedAt: instant(p.Ended), ExpiresAt: instant(p.Expiry), RedactedSummary: "owned"})
		if err != nil {
			panic(err)
		}
		err = owner.SaveEpisode(activity.Episode{ID: "owned", Title: "owned", Scope: "agent",
			StartedAt: instant(p.Started), EndedAt: instant(p.Ended), ExpiresAt: instant(p.Expiry), Confidence: 0.5})
		if err != nil {
			panic(err)
		}
		if err = json.NewEncoder(os.Stdout).Encode(map[string]any{"seeded": true}); err != nil {
			panic(err)
		}
		return
	}
	if os.Args[1] != "apply" {
		panic("unsupported oracle mode")
	}
	var result activity.RetentionResult
	switch p.Operation {
	case "expire":
		result, err = owner.Expire(instant(p.At))
	case "range":
		result, err = owner.ClearTimeRange(instant(p.Start), instant(p.End))
	default:
		panic("unsupported retention operation")
	}
	message := ""
	if err != nil {
		message = err.Error()
	}
	if err = json.NewEncoder(os.Stdout).Encode(map[string]any{"id": p.ID, "segments": result.Segments,
		"episodes": result.Episodes, "error": message}); err != nil {
		panic(err)
	}
}
