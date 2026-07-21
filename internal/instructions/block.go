package instructions

import (
	"strings"
)

const (
	// BeginMarker is the start delimiter for a managed block in a target file.
	BeginMarker = "<!-- symbrain:begin -->"
	// EndMarker is the end delimiter for a managed block in a target file.
	EndMarker = "<!-- symbrain:end -->"
)

// Render replaces (or appends) the managed block in target with the given
// content.  The result preserves everything outside the markers verbatim.
// When no markers exist the block is appended at the end of the file.
// Running Render twice with the same content produces byte-identical output
// (idempotency).
func Render(target, content string) string {
	idx := strings.Index(target, BeginMarker)
	if idx == -1 {
		// No managed block exists — append at the end.
		if target != "" && !strings.HasSuffix(target, "\n") {
			target += "\n"
		}
		return target + BeginMarker + "\n" + content + EndMarker + "\n"
	}

	endIdx := strings.Index(target[idx:], EndMarker)
	if endIdx == -1 {
		// Begin marker found but no end marker — treat everything after
		// the begin marker as the managed block region and overwrite it.
		prefix := target[:idx]
		suffix := ""
		return prefix + BeginMarker + "\n" + content + EndMarker + "\n" + suffix
	}

	// Both markers found — replace the block between them, preserving
	// the prefix (everything before the begin marker) and the suffix
	// (everything after the end marker).
	absEndIdx := idx + endIdx + len(EndMarker)
	prefix := target[:idx]
	suffix := target[absEndIdx:]

	// Ensure suffix starts on a fresh line if there's content.
	if len(suffix) > 0 && !strings.HasPrefix(suffix, "\n") && !strings.HasPrefix(suffix, "\r\n") {
		suffix = "\n" + suffix
	}

	return prefix + BeginMarker + "\n" + content + EndMarker + suffix
}
