package instructions

import "strings"

const (
	// BeginMarker is the start delimiter for a managed block in a target file.
	BeginMarker = "<!-- symbrain:begin -->"
	// EndMarker is the end delimiter for a managed block in a target file.
	EndMarker = "<!-- symbrain:end -->"

	// EscapeSentinel prefixes every escaped reserved token in managed content.
	// It is itself encoded before the begin and end markers, making the token
	// encoding a prefix code rather than a doubling convention.
	EscapeSentinel = "<!-- symbrain:escape -->"
	// EscapedEscapeSentinel is the on-disk spelling of a literal escape sentinel.
	EscapedEscapeSentinel = EscapeSentinel + "e"
	// EscapedBeginMarker is the on-disk spelling of a literal begin marker.
	EscapedBeginMarker = EscapeSentinel + "b"
	// EscapedEndMarker is the on-disk spelling of a literal end marker.
	EscapedEndMarker = EscapeSentinel + "d"
)

// Render replaces (or appends) the managed block in target with the given
// content. The result preserves everything outside the markers verbatim.
// When no markers exist the block is appended at the end of the file.
// Running Render twice with the same content produces byte-identical output.
//
// The two block delimiters are reserved in managed content. To keep arbitrary
// instruction text idempotent, literal delimiters in content are written with
// a prefix-code escape; source content itself is never modified.
func Render(target, content string) string {
	managed := escapeManagedContent(content)
	idx := strings.Index(target, BeginMarker)
	if idx == -1 {
		if target != "" && !strings.HasSuffix(target, "\n") {
			target += "\n"
		}
		return target + BeginMarker + "\n" + managed + EndMarker + "\n"
	}

	endIdx := strings.Index(target[idx:], EndMarker)
	if endIdx == -1 {
		prefix := target[:idx]
		return prefix + BeginMarker + "\n" + managed + EndMarker + "\n"
	}

	absEndIdx := idx + endIdx + len(EndMarker)
	prefix := target[:idx]
	suffix := target[absEndIdx:]
	if len(suffix) > 0 && !strings.HasPrefix(suffix, "\n") && !strings.HasPrefix(suffix, "\r\n") {
		suffix = "\n" + suffix
	}
	return prefix + BeginMarker + "\n" + managed + EndMarker + suffix
}

func escapeManagedContent(content string) string {
	// Encode the sentinel first. Each emitted token starts with the sentinel,
	// and its one-byte suffix identifies the token kind, so all token streams
	// remain distinct even when the source already contains escape tokens.
	content = strings.ReplaceAll(content, EscapeSentinel, EscapedEscapeSentinel)
	content = strings.ReplaceAll(content, BeginMarker, EscapedBeginMarker)
	content = strings.ReplaceAll(content, EndMarker, EscapedEndMarker)
	return content
}
