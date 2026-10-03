# Author prototype checks, not final clean-source acceptance

These receipts preserve initial parser/build errors and a regression-test
expectation failure. The high-risk/no-warning request requires confirmation
in the unchanged Go/kernel policy, so its audit failure correctly produces an
audit denial; the test expectation was corrected. The first strict-lint run
rejected a report function exceeding its line limit; configuration description
was extracted without suppressing the lint. The fixed strict run passed.

The 63 process comparisons and two actual lossy-output mutants passed against
uncommitted source before the harmless report extraction and additional shared
consumer test. Their captured dirty status is intentional. Final clean-source
checks and provenance are published separately. No original Go or frozen case
was changed. The prototype Unix results do not assert Windows acceptance.
