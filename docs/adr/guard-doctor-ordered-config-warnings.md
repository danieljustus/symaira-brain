# Guard Doctor ordered configuration warnings

## Decision and reason

Use the existing pinned TOML parser's immutable source spans to reproduce
BurntSushi metadata for unknown keys after successful typed decoding. The
approved parent `1c04ad92f9537d5b4d552d080433635c5f0c5403` still delegated
fourteen actual healthy Go configurations that only needed ordered warnings.
Those original input bytes and ten repeated multi-warning Go runs are retained.
This positive slice admits their full reports; it does not turn unknown keys
into configured policy fields or relax known-field type checks.

A tree traversal alone changes declaration order when a parent table is
explicitly declared after its child. A set loses duplicate metadata entries
inside arrays. Collect assignment and explicit table-header events with their
source spans, retain repeated array paths, and order events by source position.
Implicit parents of dotted keys are not separate metadata events. Dynamic
`defaults` names are decoded map keys and do not warn. Struct field aliases
matched by Go's case folding remain conservatively delegated until typed
resolution and duplicate precedence are proved; silently ignoring an alias
could hide a configured value.

Render each key segment using BurntSushi's TOML-key representation, then reuse
Core's Go quoting for the complete dotted key. Reuse Core GoText for the raw
configuration path. Human stderr retains malformed Unix path bytes directly;
no lossy String projection, JSON encoding or HTML rewriting is introduced.
No new dependency or Guard→Brain/Managed edge is needed. Guard remains the
per-call policy boundary; Brain's compatibility adapter uses the same handler.

## Output and admission ownership

Frozen Go emits warnings only after the entire typed TOML decode succeeds,
before semantic validation. Consequently a warning plus semantic validation
error has warning stderr and an error report, while a typed failure has no
warning. Native carries buffered warnings alongside configuration/report data.
It writes warning stderr once before report stdout only when the whole native
invocation has been admitted. A later malformed discovery, audit/anchor read
boundary or map-order validation gate discards both native streams, allowing
Brain's Go delegation to emit its own single warning/report. Standalone has no
Go executor and retains its explicit unsupported-state diagnostic.

This preserves final stream bytes and ordering of emission. It does not claim
identical wall-clock timing or cross-stream scheduling while native discovery
runs. The original fifty multiple-invalid-default Go observations/two reports
remain unchanged; no nondeterminism exception or E-002 interpretation is added.

## Verification and remaining scope

The permanent actual-process gate retains the exact original fourteen inputs,
adds quoted/control/Unicode/raw-path, dotted/implicit/late-table, inline/AoT and
repeated-array metadata, dynamic-default, semantic-error and typed/delegation
boundaries, and repeats the multi-warning input ten times against actual Go.
Five real process mutations drop, reorder and deduplicate warnings, leak one
before typed admission, or prepend a duplicate during an actual Go delegation.
Each must be rejected at its intended warning boundary.

The exact Brain adapter is compiled into a portable consumer test. Its actual
child processes check healthy and semantic warning output, raw Unix path bytes,
and empty native stdout/stderr before typed or discovery delegation. This is
consumer execution of the shared handler, not fresh acceptance of the entire
Brain executable. Existing decision/raw/kernel/capability/audit behavior and
frozen Go/fixtures are unchanged.

All previous124/80/94/78/63/31/35/raw4/Unicode4/audit6 and original eight mutant
controls remain required. Only the three specifically proved unknown-key inputs
in the current124/additive78 corpora are promoted to full warning/report byte
comparisons; their input bytes are unchanged. Every remaining decoder/map-order
refusal assertion stays in force. The new54-case Linux warning corpus has46
full comparisons and8 explicit typed/alias/discovery/map-order gates. Windows
runs52 launchable cases; the two raw Unix path forms are explicit inapplicable
cases rather than surrogate acceptance. Native exact-head Linux/macOS/Windows
CI and independent review remain required. Full #770/#769 and Brain's other
configuration consumers remain open.

The complete different-author review of successor source5bb independently
repeats the full warning and generated-path gates, all old warning inputs/orders
and every real mutation control. It approves this bounded implementation;
complete proof is retained in
`migration/evidence/guard-doctor-config-paths-770/independent-5bb`.
Current-head native-three-OS and protected CI remain merge requirements.
