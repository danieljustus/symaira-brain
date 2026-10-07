# Guard fixture admission to the native filesystem

The complete PR801 macOS ARM job 111412706458 rejects the owned filename
`origin-\xe2\x82` with Darwin EILSEQ92 at `mkdir`, before a product child starts.
The Guard805 e13 fixtures use the same invalid UTF-8 component. Their failure
would test fixture construction rather than Guard's behavior. The complete
independent e13 review, original job log and seven ZIP members are retained
losslessly in `migration/evidence/guard-standalone-770/kernel-fixture-admission`.

We retain the requested input domain and separately measure the domain that
the actual kernel admits. Before any raw-path product child starts, an owned
probe attempts every exact component, records the attempted path bytes,
kernel/Python identity, complete exception, errno, created names and cleanup.
The only admissible unavailable classification is **actual Darwin EILSEQ92
for an invalid UTF-8 component**. Other operating systems, errors or valid
Unicode components remain fatal fixture failures. This is a filesystem input
boundary; it changes no product parser, diagnostics or exit contract.

Every original requested case and control must appear exactly once as executed
or explicitly unexecuted. Unavailable observations record no Go/native result,
zero product children and `parity=false`. The receipt distinguishes
`original_requested_domain_complete=false` from
`platform_admitted_complete=true`. A native gate may succeed for that admitted
domain only when all admitted original comparisons and controls pass and every
unavailable input has its full probe and cleanup evidence. A corrupted ledger
that drops one requested observation must fail. This control checks exhaustive
accounting; it is never reported as a product mutation rejection.

Linux retains all 63 raw-path and 25 config-path cases and both original lossy
output controls. Darwin retains every admitted case, including ordinary paths,
valid Unicode and owner selection. Its two original lossy controls require the
invalid E2 82 filename; if that exact input is unavailable, both remain
**UNEXECUTED**, with no claimed rejection or replacement proof. A later raw
content or environment contrast must independently demonstrate a reachable
frozen-Go/native contract before it can count as product evidence. Windows
keeps its existing, separate native UTF-16 domain and bounded owner contract.

The immutable e13 generation and its complete original evidence remain intact.
Case generation, comparison predicates, owner correction, discovery parent
preparation, canonicalization control and five-second child budgets are
unchanged. New portable units and isolated Python corruption checks verify
fixture machinery; Linux mkdir probes verify only local kernel admission.
They do not establish current Guard, Darwin or Windows acceptance. Native3,
protected checks and the remaining #770/#769 contracts stay open.

The inherited observer unit also intercepted Windows `cmd /c mklink /J` with
a callback intended only for a product invocation. That host API supplies no
product `env` argument; the exact original callback raises `KeyError('env')`.
The original source and a complete actual Python callback trace are retained.
The unit now mocks directory-link construction separately and verifies its
exact inputs. An additional pure Windows-API mock verifies the real helper's
argv and kwargs. This does not simulate a successful Guard invocation or prove
Windows junction behavior. The real junction helper and full owner mutation
control remain unchanged. Invalid-byte probe units have only their explicit
Unix filename domain; valid Unicode and all pure accounting tests remain
mandatory on every platform.
