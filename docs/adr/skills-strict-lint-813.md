# Skills strict lint admission after the first native failure

The first macOS source checkpoint recorded 23 strict pedantic Clippy
diagnostics in `symbrain-skills`. Its original raw log and source snapshot
remain in the preserved source checkpoint and are not part of the PR. The
correction was authored against immutable
`b49372bedcd9c4514706c999447dc4f836be012a`. It is integrated with current
main here; new strict Clippy results are reported separately from the
historical source-only checks.

We fix warning owners where doing so preserves their contracts. A bundle
load binding is renamed; an unused import is removed; the comment on an
`include!` becomes a normal comment; documentation uses code spans and states
its existing panic conditions. Explicit map defaults, an unnecessary borrow
and a trailing format comma are cleaned up. Context iteration enumerates the
same slice from the same index, preserving first-match order. Variant dispatch
uses `if let` with the same UTF-8 success and failure bodies, including all
raw-byte returns and marker errors.

The bisect grammar width takes only values 1 and 4. Giving it a byte type and
using widening conversions eliminates a hypothetical truncation without
changing any trie, pattern, EOF, integer or error semantics. The Unix path
conversion takes the low byte with `to_le_bytes()[0]`: that equals the previous
`u16 as u8` for every possible unit, including values outside the actual Unix
byte domain. It adds no panic and does not normalize bytes or Windows units.

Three specific lint exceptions retain established contracts. The public
installer's independent booleans remain source-compatible rather than being
replaced by a new options API. The bounded newline-count statement retains
its byte scan and avoids a new dependency. Only the archive extractor retains
`Debug` formatting: changing it to `Display` would remove quotes and escaping
from existing path diagnostics. Its two format arguments are inlined while
keeping their exact `:?` conversion. No crate-wide lint rule changes.

All production outside these 17 warning-owner files, frozen Go, tests,
corpora, manifests, lockfile, workflows, timeouts, comparator assertions and
previous source/error controls remain byte- and mode-identical to the parent.
A pure token projection and exhaustive mathematical byte/width controls bind
these edits to their original bodies; they do not execute native Rust or Go.
Standalone Rustfmt on the changed entries and unchanged workflow actionlint
are source checks only. This preparation neither claims strict Clippy success
nor accepts the 1112/1070 native differential domain. Fresh strict compilation,
all existing affected/workspace tests and native three-platform differential
gates remain required after independent review and resource allocation.
