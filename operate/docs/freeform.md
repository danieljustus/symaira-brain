# Freeform adapter

This optional, local Operate adapter supports Freeform 5.0. GUI writes are
explicitly gated. The Phase 2 helper creates private snapshots of the live store
only when requested through the user-registered launchd job. It never starts
Freeform, requests TCC permissions or installs itself. All tests use synthetic data.

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
* `freeform_read_board`: absolute `snapshot_path` and required `board_identifier`
  UUID. Requires `capture`. Known Freeform live-storage paths, including symlink
  targets, are refused. The caller remains responsible for snapshot provenance.
* `freeform_list_boards`: absolute `snapshot_path`; metadata only, with `id`, raw
  `last_activity_time` when present and non-tombstoned `item_count`. Requires `capture`.
* `freeform_snapshot`: one-shot launchd helper backup; requires `capture`. See Phase 2.

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

Object kinds: `shape`, `sticky_note`, `text`, `image`. The sticky-note representation is
the observed exported shape plus curved shadow, not a newly invented native
sticky-note type. Text uses the exported unfilled shape variant. RGB is normalized
0...1 and affects shape/note fill; free text has no fill. Width/height for free text
retain the reference's automatic-sizing flags and may be recomputed by Freeform.

`image` objects need `image_path`, an absolute path to a PNG or JPEG (magic bytes
checked, at most 25 MB each, 100 MB per call); `rgb` is ignored. `text` becomes the
image description, which Freeform exposes as the AX label, so a unique `text` makes
the image addressable by `freeform_move`. Mirroring the Freeform 5.1 export, the
payload references the bytes through `resource.indirect.identifier`; the bytes are
written as that extra type on the same pasteboard item. Live-verified on macOS 27.2
with PNG and JPEG, including an image as connection endpoint.

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
snapshot or touch the live store itself. It filters tombstones and requires a board
UUID for object contents, parses `crdt` + uint32 LE 6, extracts the observed two float-pair geometry,
and returns only the first user-looking text line. This is a bounded heuristic,
not a complete attributed-text decoder; later lines and runs are intentionally
omitted to avoid attribute debris. Connections report unordered UUID occurrence
evidence, not proven head/tail orientation. Containers and unknown item types are
reported without inventing semantics. Limits: 10,000 items, 16 MiB per blob/row,
64 MiB total selected blob bytes, protobuf nesting depth 8.

## Phase 2: independently authorized snapshot helper

The separate `symoperate-freeform-snapshot` executable links only Foundation and
SQLite3 through `FreeformSnapshotCore`. It accepts no arguments, reads only
`~/Library/Group Containers/group.com.apple.freeform/Boards/boards.db` via a
read-only SQLite URI and the SQLite backup API, including committed WAL content.
It does not copy the DB file blindly, write the source, start Freeform or use the
network. One invocation creates one snapshot, then exits.

The fixed destination is
`~/Library/Caches/com.symaira.operate/freeform-snapshot/` (0700).
A private temporary DB is converted to `journal_mode=DELETE` and must pass
`PRAGMA quick_check` before an atomic rename publishes `boards.db` (0600).
Only then is `status.json` (0600) atomically published. It contains `ok`, Unix
`created_at` seconds, source DB `source_mtime`/`source_size`, snapshot `bytes`,
`helper_version`, and an error on failure. Source metadata describes the main DB,
not the WAL. A failed backup preserves the old snapshot but publishes failure;
callers must not mistake that old file for fresh success. A failure to create the
cache/status file itself is reported on stderr and produces a nonzero exit.

### Packaging and user registration

Run from the repository root yourself:

```bash
export SYMAIRA_EXTERNAL_RUNTIME_ROOT=/Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/builds/symaira-brain/.worktrees/freeform-961/runtime
operate/scripts/freeform-snapshot-helper.sh \
  --sign 'Developer ID Application: Daniel Justus (M4744F3TAA)'
launchctl bootstrap gui/$(id -u) "$HOME/Library/LaunchAgents/com.symaira.operate.freeform-snapshot.plist"
```

The script builds Release, installs the app at
`~/Applications/Symaira/Symaira Operate Freeform Snapshot.app`, and writes the
LaunchAgent plist. It only **prints** the bootstrap command and never runs it.
In System Settings > Privacy & Security > Full Disk Access, add and enable that
exact app. The helper needs FDA, not the agent host. Stable Developer ID signing
preserves the helper identity across updates. Signing uses hardened runtime and
a timestamp, but not notarization. Unsigned packaging is supported for CI/tests;
Swift tests never require a signing identity.

The on-demand job label and bundle ID are
`com.symaira.operate.freeform-snapshot`; `AssociatedBundleIdentifiers` names that
bundle, and `ProgramArguments` contains only its installed executable. There is
no RunAtLoad or KeepAlive. Launching as an agent child, with `open`, or by sending
AppleScript to Terminal does not provide independent FDA authorization and is
not a supported fallback. The user must register the job themselves once.

`--dest /absolute/build/path` stages the app and `LaunchAgents/*.plist` entirely
there, without installation or any launchctl call. `--uninstall` boots out the
registered job and removes only the installed bundle/plist; snapshots remain
until explicitly deleted. `--dest DIR --uninstall` removes only staged artifacts
and never calls launchctl.

### MCP and privacy contract

`freeform_snapshot` invokes `/bin/launchctl kickstart -p
 gui/<uid>/com.symaira.operate.freeform-snapshot` using the bounded process runner,
not a shell. It then waits for `status.json.created_at >= request start`.
The default total deadline is 15 seconds; `timeout` accepts >0 through 60 seconds.
Missing registration returns setup instructions, helper failure reports FDA/error
status, and stale/missing status produces a timeout. The success response has
`ok`, `snapshot_path`, `created_at`; failure has `ok:false` and `error`.
`freeform_doctor` passively adds `helper_installed` (plist existence, not proof of
registration) and `helper_last_status` when a bounded, valid status file exists.
It never starts a job.

**The snapshot contains ALL boards, including content not selected for output.**
It is local private cache data, not a per-board export. `capture` and the scoped
Freeform grant apply before starting the helper or reading metadata/content.
Use `freeform_list_boards` with the returned path to choose an ID, then
`freeform_read_board` with an explicit `board_identifier`. Unscoped object reads
now fail closed, a deliberate privacy-breaking change from Phase 1. Alternatively,
`freeform_snapshot` with `read:true` and `board_identifier` returns the existing
read-board result directly after fresh successful backup; a failed backup returns
its error rather than old content.

Board listing selects IDs/activity/counts only and includes empty boards. Missing
activity metadata is omitted rather than guessed; activity values retain SQLite's
stored units. No board title is returned: the `boards.data` title field/encoding
has not been evidenced, and arbitrary CRDT text must not be presented as a title.
Synthetic fixtures ensure content blobs and plausible strings never leak through
listing. No production database or private board contents are fixtures.

Freeform can take approximately 10 seconds to persist inserted objects. A fresh
SQLite snapshot proves committed store state, not that pending UI changes have
already reached the store. Retry after persistence when necessary. Mock runner,
synthetic WAL, bundle structure and signing checks are not live TCC acceptance.

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
