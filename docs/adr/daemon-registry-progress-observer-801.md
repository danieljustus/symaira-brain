# Registry capture observer checks the complete four-stage journal

The four actual native CI jobs111449295271/369/377/380 failed in the Python
journal selftest at test_registry_progress.py:67 before the following daemon
process/race gates. Eight owned Python captures already recorded cli.begin,
cli.launch.begin, cli.spawned and cli.end, plus the initial binding. The test
still expected the former two-stage17-record sequence. The complete original
logs and source are preserved before correction; no journal duplication was
observed or inferred from this assertion failure.

The observer now requires exactly33 complete newline records with contiguous
sequence1..33 and monotonic event timestamps. Every one of the eight distinct
cases has the four stages exactly once, in source order, with no foreign case
link. Binary and original argv bytes, launch/spawn working directory, unchanged
15-second child budget, positive PID linked to completion, successful status,
absence of timeout/failure/cleanup errors, and complete raw result-receipt and
two stream paths/bytes/SHA/base64 witnesses are checked. Per-case ordering is
required while interleaving between different cases remains unrestricted.

This preserves the diagnostic contract instead of reducing the assertion to
an approximate event count. The production Progress journal, file-backed
capture, registry/parity comparisons, child15s/cleanup2s deadlines, native
input families, platform admission and Windows endpoint owners are unchanged.
Nothing here turns an unavailable pair or interrupted gate into parity.

Five actual Python-only selftests run under a bounded owned subreaper. A
separate retained ordinary eight-child run records33 events. Two meaningful
concurrency controls then delete every launch witness or corrupt every end's
case link. The former has25 records and fails the exact sequence; the latter
still has33 but fails linkage. All eight children in each control really ran
and exited0; complete journals, receipts and raw streams remain in the proof
archive. Rejection therefore comes from the intended observer defects rather
than a failed fixture or product. The complete original five-test run also
reproduces its old17/33 assertion, preserving that actual failure.

These are owned Python machinery checks on Linux, not Go/Rust product or native
Windows/macOS acceptance. No SDK/compiler/product/target/cache/provider/port
was used. All four original jobs and the older retained native failures stay
historical; fresh full native/protected acceptance remains required. #772 and
all wider cutover contracts remain open. A different author must review this
source-only successor before publication.
