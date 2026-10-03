# Credentialed usage acceptance (#620)

`run.sh OUTPUT_JSON` runs supplemental test harnesses against immutable Go
commit `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`, with its pinned Go 1.26.7
toolchain. Only two new test files are added to an owned disposable worktree;
all existing Go source and oracle fixtures remain unchanged. No operator
credentials, keychain reads, external endpoints or process probes are used.

The matrix contains 133 reports: 19 provider/strategy variants and seven
responses each (success, 401, 403, 429 with/without `Retry-After`, 500 and
malformed JSON). It covers all ten providers, including Antigravity with
synthetic process/port observations and the real production request walk.
Claude and Kimi include complete failing fallback chains. Copilot covers
github.com and explicit enterprise authorities, including a port. Kimi CLI
covers all five identity fields, with and without a stored device id.

Native tests exercise the production resolved-credential constructors,
request/fetch methods and report assembly. Their actual reports and requests
are retained alongside the Go results. The actual native CLI output renderer
then compares JSON and table bytes against the unchanged Go CLI renderers:
266 output comparisons. Only `snapshot.fetched_at` is set to the Unix epoch;
OpenCode's random `X-Server-Instance` is checked for its `server-fn:` shape.
Fresh native runs compare actual host/platform identity verbatim, including
an intentionally incorrect `COMPUTERNAME` environment value. The retained
local fixture substitutes only host/platform identity when replayed elsewhere.

Kimi's OS-version/model fields preserve the compatibility baseline's `1.26.7`
SDK identity. Go reports its runtime version there, not an OS version. The
native value is deliberately independent of an installed Go executable.
Windows uses `hostname = 0.4.2` only on that target: its safe API wraps the
same `GetComputerNameExW(ComputerNamePhysicalDnsHostname)` query as Go.
Existing dependency versions remain pinned; the new dependency is MIT.

Copilot's builder accepts an explicit enterprise host. The default CLI
constructor continues to select github.com because Go does not populate its
enterprise field from environment variables or credential-file keys. HTTPS,
private-address and ambiguous-authority validation remains in the transport.

The acceptance runner also executes four real negative controls: missing
oracle, altered Kimi identity, altered JSON output and incomplete matrix. Each
must fail with the corresponding diagnostic; a zero-test run is rejected.
Receipts bind the candidate commit, Go checkpoint, harness hashes, raw reports,
requests and controls. Existing three-OS migration jobs execute this gate and
retain its evidence for 14 days. Native macOS/Windows acceptance is pending
until those jobs succeed at the published candidate head.

The broader #768 cutover remains open. `needs_go_fallback` still delegates
secret references, Claude Keychain-only credentials, malformed/aliased or
ambiguous credential files, non-ASCII Kimi device ids, unsupported URL/workspace
forms and differing Windows home roots. The exact source restrictions remain
documented at the CLI gate; this change does not remove them.
