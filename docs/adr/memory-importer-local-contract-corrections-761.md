# Memory761 local byte, path and deletion-boundary corrections

Status: source-only correction of three independently reviewed P2 findings.
There is no compiled/runtime acceptance, new routing admission or issue closure.
The user's delegated product authority permits these reversible corrections.

## Preserve the rejected checkpoint

Original publication `2946cf2b959ed71ed58dcf44d36c515aab60328e` and production
`734d2705f6b9312599644700c845db04f07d685c` remain clean and unchanged. The complete
different-author REQUEST CHANGES report, receipt and 3568-file raw preservation
manifest are retained losslessly and referenced in
`migration/evidence/memory-importers-761/local-corrections/original-review-retention.json`.
The findings were source-derived counterexamples, not observed runtime failures.
All original 70 constructor recipes, 14 frozen fixtures, three process controls,
24 unit functions and source-map/case-plan bytes remain historical proof. The
one incorrect invalid-byte JSON test expectation is corrected in this successor;
its original bytes remain in the rejected checkpoint and original review archive.

This isolated successor uses a complete parent Git index and a sparse checkout
under the explicitly allocated 15 MB source budget. Missing checkout files are
still represented by their exact parent blobs; they are not removed or waived.
The full runtime workspace must be materialized after a separate allocation.

## Keep each correction at its owner

Activity Expire and ClearTimeRange now bind a private UTC database-time formatter.
Pinned modernc1.59 `conn.formatTime` defaults to `time.Time.String`; CoreKit0.17's
sqlitekit DSN supplies no write-time override. The pinned SDK renders nine
fractional digits and trims trailing zeroes. The prior inherited `gotime::format`
truncated to microseconds: an expiry ending in 500 ns was incorrectly retained
at 600 ns, and an interval ending in 500 ns was incorrectly deleted by a range
starting at 600 ns. This changes deleted rows, so adapting only output would be
incorrect. The private formatter retains the SDK year width/sign, UTC suffix
and nanoseconds. The inherited formatter and unrelated Store reads/writes remain
unchanged and require their own existing gates. Chrono-only leap-second values
cannot be represented by Go UTC time; these new mutations refuse them before
opening the SQL transaction, with zero deletion counts. This is a documented
bounded refusal, not proof of the entire chrono/Go timestamp domain.

The importer metadata JSON encoder consumes raw bytes until encoding. Go's
`encoding/json.appendString` writes ASCII `\ufffd` for each malformed UTF-8
byte, but copies a valid literal U+FFFD as UTF-8. Converting both into Unicode
first erased a visible distinction in embedded frontmatter/link JSON. The local
encoder keeps that distinction, ASCII controls, HTML and U+2028/U+2029 escapes,
valid Unicode and raw-byte key ordering. Neither paths nor content are globally
normalized, and no shared JSON/DTO/provider policy changes.

The local Unix Base helper follows `filepath.Base` directly: empty input becomes
dot, trailing separators are stripped, and the final component is returned
without Clean. Explicit Obsidian roots ending in `/.` or `/..` retain their vault
name; the shell command `npm/. install` is not tagged as a package-manager call.
`npm/ install` remains tagged. Existing Clean/Join/Rel and symlink lookup ownership
are unchanged. No canonicalization or Core/installer dependency is introduced;
non-Unix path contracts remain explicitly gated pending their own native proofs.

## Prepared evidence, with execution still pending

`corrections.py` materializes the original 70 recipes unchanged plus 23 additive
real-constructor cases: three families' malformed/literal/JS/key metadata,
invalid/literal links, Obsidian dot/parent/trailing/raw roots and shell command
components. The original literal full-report comparator is unchanged. The new
retention Go caller imports actual frozen DB and Activity owners, seeds with
SaveSegment/SaveEpisode, and invokes Expire/ClearTimeRange. Its native peer uses
the existing public shared Store methods on private clones of those Go-seeded
databases. Twelve nanosecond/equal/whole-second/UTC-conversion cases compare the
entire result, error and all table/schema/row values; there is no expected-data
copy of the deletion algorithm.

`correction_gate.py` prepares two actual-process input controls (dot-root owner
corruption and a whole-second query boundary) plus one byte-corruption control
over a successful actual native frontmatter report. Child/bootstrap failures
cannot count as a detected mutation. The three original controls remain required
as well. Eight additive unit functions cover deletion rows/counts, equality,
timestamp formatting/refusal, invalid versus valid replacement, raw key order,
Base/Join independence and the real shell tag consumer: 32 total functions are
prepared, not passed.

No Cargo/compiler, SDK behavior probe, product, provider, endpoint, target or
port is allocated or executed for this correction. Standalone source formatters,
AST/shell/hash/index checks are allowed and separately recorded. Different-author
full source review, actual complete constructor/retention/control/readonly
process results, unchanged Memory/Activity/CLI/MCP parent gates, strict Clippy and
native three-OS CI remain required. Registry/engine governance, ten other importer
families, other Activity mutation APIs and all previously documented admission
limits remain pending. No new route is opened.
