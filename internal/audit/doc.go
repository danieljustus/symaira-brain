// Package audit writes the JSONL audit log for tool calls made through the
// gateway, applying redaction rules so secrets and sensitive arguments are
// never persisted.
package audit
