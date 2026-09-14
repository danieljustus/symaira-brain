package main

import (
	"encoding/base64"
	"encoding/json"
	"os"
	"time"

	"github.com/danieljustus/symaira-brain/guard/internal/audit"
)

type parityFixture struct {
	Audit     string `json:"audit_base64"`
	Response  string `json:"response_base64"`
	Timestamp string `json:"timestamp"`
}

type response struct {
	Decision string `json:"decision"`
	Reason   string `json:"reason"`
}

func main() {
	timestamp := time.Date(2026, 9, 14, 12, 0, 0, 123456789, time.FixedZone("plus2", 2*60*60))
	record := audit.ExternalDecision{
		ID:        "evt_1_decide_1",
		Command:   "<&>\u2028\u2029",
		RiskClass: "low",
		Domain:    "example.test",
		Warnings:  []string{"<&>\u2028\u2029"},
		Decision:  "allow",
		Reason:    "<&>\u2028\u2029",
		DecidedAt: timestamp.UTC().Format(time.RFC3339),
	}
	responseBytes, err := json.Marshal(response{Decision: "allow", Reason: "<&>\u2028\u2029"})
	if err != nil {
		panic(err)
	}
	auditBytes, err := json.Marshal(record)
	if err != nil {
		panic(err)
	}
	fixture := parityFixture{
		Audit:     base64.StdEncoding.EncodeToString(auditBytes),
		Response:  base64.StdEncoding.EncodeToString(responseBytes),
		Timestamp: record.DecidedAt,
	}
	encoder := json.NewEncoder(os.Stdout)
	encoder.SetEscapeHTML(false)
	if err := encoder.Encode(fixture); err != nil {
		panic(err)
	}
}
