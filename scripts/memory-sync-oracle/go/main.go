// Supplemental owned probe. Copy only this file into a private package inside
// an unchanged copy of frozen dcddcef0 before building. No implementation is
// substituted for the actual syncclient, database or crypto engine.
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
	"github.com/danieljustus/symaira-brain/internal/memory/syncclient"
)

type input struct {
	Operation     string             `json:"operation"`
	Database      string             `json:"database"`
	Remote        string             `json:"remote"`
	Token         string             `json:"token"`
	Pull          bool               `json:"pull"`
	Push          bool               `json:"push"`
	Relay         bool               `json:"encrypted_relay"`
	Passphrase    string             `json:"passphrase"`
	AllowInsecure bool               `json:"allow_insecure_http"`
	Timeout       time.Duration      `json:"timeout_ns"`
	PageLimit     int                `json:"page_limit"`
	Since         time.Time          `json:"since"`
	LastID        string             `json:"last_id"`
	Memories      []*db.Memory       `json:"memories"`
	Deleted       []db.DeletedMemory `json:"deleted"`
	SetupSQL      []string           `json:"setup_sql"`
	Query         string             `json:"query"`
	Blob          []byte             `json:"blob"`
	Plaintext     []byte             `json:"plaintext"`
}

type output struct {
	Value any    `json:"value"`
	Error string `json:"error,omitempty"`
}

func main() {
	var request input
	if err := json.NewDecoder(os.Stdin).Decode(&request); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(2)
	}
	value, err := execute(request)
	response := output{Value: value}
	if err != nil {
		response.Error = err.Error()
	}
	if err := json.NewEncoder(os.Stdout).Encode(response); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func execute(request input) (any, error) {
	switch request.Operation {
	case "validate_url":
		return nil, syncclient.ValidateRemoteURL(request.Remote, request.AllowInsecure)
	case "encrypt":
		return security.NewCryptoEngine().Encrypt(request.Plaintext, request.Passphrase)
	case "decrypt":
		return security.NewCryptoEngine().Decrypt(request.Blob, request.Passphrase)
	}
	cfg := config.Defaults()
	cfg.Database.Path = request.Database
	database, err := db.Open(cfg)
	if err != nil {
		return nil, err
	}
	defer func() { _ = database.Close() }()
	for _, statement := range request.SetupSQL {
		if _, err := database.Conn().Exec(statement); err != nil {
			return nil, err
		}
	}
	switch request.Operation {
	case "run":
		return syncclient.Run(context.Background(), syncclient.Options{
			Remote: request.Remote, Token: request.Token, Pull: request.Pull, Push: request.Push,
			EncryptedRelay: request.Relay, Passphrase: request.Passphrase,
			AllowInsecureHTTP: request.AllowInsecure, DB: database,
			PageLimit: request.PageLimit, Timeout: request.Timeout,
		})
	case "cursor":
		return database.GetSyncCursor(request.Remote)
	case "set_cursor":
		return nil, database.SetSyncCursor(request.Remote, request.Since)
	case "memories":
		return database.GetMemoriesSinceCursorIDForSync(request.Since, request.LastID, request.PageLimit)
	case "deleted":
		return database.GetDeletedSinceCursorID(request.Since, request.LastID, request.PageLimit)
	case "upsert":
		var applied []bool
		for _, memory := range request.Memories {
			ok, err := database.SyncUpsertMemoryIfNewer(memory)
			if err != nil {
				return applied, err
			}
			applied = append(applied, ok)
		}
		return applied, nil
	case "delete":
		var removed []bool
		for _, deleted := range request.Deleted {
			ok, err := database.ApplyRemoteDelete(deleted.ID, deleted.DeletedAt)
			if err != nil {
				return removed, err
			}
			removed = append(removed, ok)
		}
		return removed, nil
	case "query":
		rows, err := database.Conn().Query(request.Query)
		if err != nil {
			return nil, err
		}
		defer func() { _ = rows.Close() }()
		columns, err := rows.Columns()
		if err != nil {
			return nil, err
		}
		var result []map[string]any
		for rows.Next() {
			values := make([]any, len(columns))
			pointers := make([]any, len(columns))
			for i := range values {
				pointers[i] = &values[i]
			}
			if err := rows.Scan(pointers...); err != nil {
				return result, err
			}
			row := make(map[string]any, len(columns))
			for i, column := range columns {
				row[column] = values[i]
			}
			result = append(result, row)
		}
		return result, rows.Err()
	default:
		return nil, fmt.Errorf("unsupported owned probe operation %q", request.Operation)
	}
}
