// Prepared owned public-client/SDK/crypto probe; not built or executed.
// Copy into an unchanged PRIVATE copy of frozen dcddcef0. All imports below
// are actual frozen owners. No frozen file, test, implementation or fixture
// is replaced. Entropy override is process-local and only synthetic.
package main

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/json"
	"fmt"
	"os"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
	"github.com/danieljustus/symaira-brain/internal/memory/syncclient"
)

type step struct {
	Operation  string `json:"operation"`
	Passphrase string `json:"passphrase"`
	Bytes      []byte `json:"bytes"`
	Reset      bool   `json:"reset"`
}

type input struct {
	Operation     string             `json:"operation"`
	Remote        string             `json:"remote"`
	Token         []byte             `json:"token"`
	AllowInsecure bool               `json:"allow_insecure_http"`
	Since         time.Time          `json:"since"`
	Cursor        string             `json:"cursor"`
	Limit         int                `json:"limit"`
	TimeoutNS     int64              `json:"timeout_ns"`
	Memories      []*db.Memory       `json:"memories"`
	Deleted       []db.DeletedMemory `json:"deleted"`
	Blobs         []db.RelayBlob     `json:"blobs"`
	Bytes         []byte             `json:"bytes"`
	Entropy       *[]byte            `json:"entropy"`
	Steps         []step             `json:"steps"`
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
	result := output{Value: value}
	if err != nil {
		result.Error = err.Error()
	}
	if err := json.NewEncoder(os.Stdout).Encode(result); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func execute(request input) (any, error) {
	// Actual encoding/json over actual exported frozen response types. This
	// does not stand in for client transport or inner Run/decode assertions.
	switch request.Operation {
	case "decode_changes":
		var value syncclient.ChangesResponse
		err := json.NewDecoder(bytes.NewReader(request.Bytes)).Decode(&value)
		return value, err
	case "decode_apply":
		var value syncclient.ApplyResult
		err := json.NewDecoder(bytes.NewReader(request.Bytes)).Decode(&value)
		return value, err
	case "decode_relay":
		var value syncclient.RelayResponse
		err := json.NewDecoder(bytes.NewReader(request.Bytes)).Decode(&value)
		return value, err
	case "decode_relay_result":
		var value syncclient.RelayPushResult
		err := json.NewDecoder(bytes.NewReader(request.Bytes)).Decode(&value)
		return value, err
	case "encode_memory":
		return json.Marshal(request.Memories)
	case "crypto_sequence":
		if request.Entropy != nil {
			rand.Reader = bytes.NewReader(*request.Entropy)
		}
		engine := security.NewCryptoEngine()
		var outputs []output
		for _, step := range request.Steps {
			if step.Reset {
				engine = security.NewCryptoEngine()
			}
			var value []byte
			var err error
			switch step.Operation {
			case "encrypt":
				value, err = engine.Encrypt(step.Bytes, step.Passphrase)
			case "decrypt":
				value, err = engine.Decrypt(step.Bytes, step.Passphrase)
			default:
				return outputs, fmt.Errorf("unsupported owned crypto operation %q", step.Operation)
			}
			result := output{Value: value}
			if err != nil {
				result.Error = err.Error()
			}
			outputs = append(outputs, result)
		}
		return outputs, nil
	}
	client, err := syncclient.NewClientWithOptions(request.Remote, string(request.Token), nil, request.AllowInsecure)
	if err != nil {
		return nil, err
	}
	ctx := context.Background()
	if request.TimeoutNS != 0 {
		var cancel context.CancelFunc
		ctx, cancel = context.WithTimeout(ctx, time.Duration(request.TimeoutNS))
		defer cancel()
	}
	switch request.Operation {
	case "changes":
		return client.Changes(ctx, request.Since, request.Cursor, request.Limit)
	case "apply":
		return client.Apply(ctx, request.Memories, request.Deleted)
	case "relay_pull":
		return client.RelayPull(ctx, request.Since, request.Limit)
	case "relay_push":
		return client.RelayPush(ctx, request.Blobs)
	default:
		return nil, fmt.Errorf("unsupported owned backend operation %q", request.Operation)
	}
}
