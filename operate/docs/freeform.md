# Freeform adapter, Phase 1

This is an optional, local Operate adapter for Freeform 5.0. It does not start
Freeform, create boards, access the live database, request TCC permissions or
install a helper. All new tests use injected services and synthetic data.

## Tools and grants

* `freeform_doctor`: running PID, Accessibility grant, accessible window count.
* `freeform_insert`: insert a `board` IR object. `dry_run: true` validates and
  returns a `DryRunPlan` without calling any system service.
* `freeform_connect`: the same transaction, requiring at least one connection.
  Both endpoint objects must be newly inserted in the same IR. Connecting an
  existing object by UUID is deliberately not supported.
* `freeform_move`: single-object AX movement with readback and one correction.
  Supply `label`, `reference_label`, `reference_x`, `reference_y`, `x`, `y` and
  explicit `zoom: 100`. Labels are unique object texts without the localized type
  suffix. Duplicate matches fail closed. The reference coordinate is its board
  top-left, not center. There is no multi-selection move.
* `freeform_read_board`: absolute `snapshot_path`, optional `board_identifier`
  UUID. The caller supplies an already-created SQLite snapshot. Requires
  `capture`. Known Freeform live-storage paths, including symlink targets, are
  refused. The caller remains responsible for snapshot provenance.

Every write tool (`freeform_insert`, `freeform_connect`, `freeform_move`) requires
`board_title`. The call fails closed before any pasteboard, focus or AX write unless
Freeform has exactly one window and its title equals `board_title` exactly. Use
`freeform_doctor` (`window_titles`) to read it. Without this check a live test once
pasted into the user's real board instead of the disposable one.

All writes require `input`, `app_control` and `menu_action`, intersected with the
startup ceiling and application-scoped grants. A dry run reports denial rather
than bypassing the grant. No capability descriptor grants execution authority.

Example `freeform_insert` arguments:

```json
{
  "board": {
    "objects": [
      {"id":"11111111-1111-1111-1111-111111111111","kind":"shape","text":"Start","x":100,"y":100,"width":140,"height":70,"rgb":[1,0.5,0]},
      {"id":"22222222-2222-2222-2222-222222222222","kind":"sticky_note","text":"Goal","x":400,"y":200,"width":200,"height":200,"rgb":[1,1,0]}
    ],
    "connections": [
      {"tail":"11111111-1111-1111-1111-111111111111","head":"22222222-2222-2222-2222-222222222222","line_end":"none"}
    ]
  },
  "board_title": "Disposable test board",
  "dry_run": true
}
```

Object kinds: `shape`, `sticky_note`, `text`. The sticky-note representation is
the observed exported shape plus curved shadow, not a newly invented native
sticky-note type. Text uses the exported unfilled shape variant. RGB is normalized
0...1 and affects shape/note fill; free text has no fill. Width/height for free text
retain the reference's automatic-sizing flags and may be recomputed by Freeform.

## Content-Language and paste transaction

The resource templates reproduce the observed export schema with all source text
and positions cleared; formatting archives are retained. See
`../SOURCE_PROVENANCE.md` for source receipts and parser attribution.

Only evidenced connection enum values are accepted: `line_end: none`,
`line_type: curved`, `pattern: solid`, `magnet: center`. `arrow` is rejected before
any pasteboard or focus operation. The final generated payload is checked again
before it is written to `com.apple.apps.content-language.canvas-object-1.0`.

The adapter serializes its transactions. It remembers the prior frontmost PID,
materializes all pasteboard items/types, writes the payload, sets Freeform's
`AXFrontmost`, polls Paste enabled for up to 40 × 50 ms, optionally presses
Deselect, presses Paste through AX, waits 300 ms, restores prior PID focus, and
restores the pasteboard with exact readback. Cleanup runs on failures too.
Menu items match command character plus the complete AX modifier mask, never
translated titles. Command is implicit; Shift=1, Option=2, Control=4,
NoCommand=8. Missing Deselect (Shift-Command-A) is safely skipped. There are no
synthetic keystrokes or `NSRunningApplication.activate` calls on this path.

`submitted`, `objects` and `connection_count` mean AX submission, not proof that
the document changed. `effect: unverifiable` is intentional until an opt-in live
readback can confirm the import. Inspect `error`, `focus_restored` and
`pasteboard_restored` independently. A restoration failure is never hidden by a
successful paste. Do not run another clipboard-writing tool concurrently. A
user changing the clipboard during this short transaction will have their
clipboard replaced by the saved contents, as required by the restore contract.

## Placement and snapshot limits

Freeform centers pasted groups in the visible viewport. Relative distances survive,
but paste is not absolute placement. Use a previously calibrated reference object
and `freeform_move` afterwards. Only a focused/sole board window and visible
objects at 100% zoom are supported. The adapter verifies exactly one selected
AXLayoutItem before setting AXPosition; background movement does not focus an app.
The correction threshold is 0.5 pt on either axis. A failed readback is not success.

The reader opens SQLite with READONLY and a read transaction. It does not create a
snapshot or touch the live store. It filters tombstones and optionally a board
UUID, parses `crdt` + uint32 LE 6, extracts the observed two float-pair geometry,
and returns only the first user-looking text line. This is a bounded heuristic,
not a complete attributed-text decoder; later lines and runs are intentionally
omitted to avoid attribute debris. Connections report unordered UUID occurrence
evidence, not proven head/tail orientation. Containers and unknown item types are
reported without inventing semantics. Limits: 10,000 items, 16 MiB per blob/row,
64 MiB total selected blob bytes, protobuf nesting depth 8.

The Developer-ID-signed, independently TCC-authorized snapshot helper, launchd,
live DB backup, signing and distribution belong to Phase 2.

## Verification safety

Build and test with the repository's external-build launcher on Daniel's Mac.
Set `SYMAIRA_EXTERNAL_RUNTIME_ROOT` to a pre-existing external test-runtime
directory. Existing CLI smoke tests expect `operate/.build/debug/symoperate`;
when using external scratch, an ignored `.build` symlink may point at that build.
InputService unit and smoke tests now inject an event-delivery sink, so the full
suite does not post synthetic desktop input. The existing permission-prompt CLI
test is opt-in via `SYMOPERATE_LIVE_PERMISSION_TESTS=1` on a dedicated desktop.

Live acceptance is separate and intentionally not run by unit tests. On a disposable
board, the parent can insert two shapes and an anchored line, move the objects at
100% zoom, inspect a separately supplied snapshot, and compare clipboard and focus
before/after. Passing mock tests is not live Freeform acceptance.
