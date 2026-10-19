# Remaining Guard doctor delegation inventory

All rows formerly marked "delegated" are rendered natively with approved,
deterministic detail text since the
[#770 deviation decision](../docs/adr/guard-doctor-approved-deviations-770.md).

Approved parent8e3 and successor source are bound by tracked receipts. The
38 real baseline probes classify Go success, Go error and native delegation;
raw original outputs, exit and private state are retained. None does not mean
an error: some valid forms formerly returned None. No wildcard waiver is used.

| Source boundary | Go behavior / reachability | Successor handling |
|---|---|---|
| config: inline tables and inline/empty struct arrays | reachable healthy and semantic-error states | admitted after all known typed fields; actual equivalence proof |
| config: known scalar/list/struct type checks | reachable decoder errors before semantic validation; ordering follows source fields | native fixed decoder message (approved deviation) |
| config: unknown root/nested keys | reachable warning-only healthy states, or warnings before semantic validation | ordered warnings admitted after typed decoding; buffered until full report admission |
| config: case-fold aliases of known fields | reachable healthy or typed/duplicate-precedence states | native decoder error, stricter than Go (approved); never treated as unknown keys |
| config: multiple invalid defaults | reachable validation error; Go map iteration chooses different invalid fields | first invalid key in key order (approved deviation);50 actual Go runs remain preserved |
| config: unsupported parser diagnostic/span/offending byte | reachable TOML syntax errors; only proven missing-equals form is currently admitted | `toml: line N:` plus the native parser message (approved deviation) |
| config: read-to-string failure | reachable read errors, directories and invalid UTF-8 TOML content; may involve stat/read stage distinctions | Go frame plus platform error text (approved deviation); Go probe retained |
| config: absent fields | healthy default values, not None | existing native handling retained |
| anchor: malformed UTF-8/unpaired surrogate keys or strings | reachable healthy decoded anchors and typed errors in following/preceding fields | admitted with existing Go JSON replacement after original syntax validation |
| anchor: empty token / missing first raw byte | unreachable after successful original syntax validation | defensive None retained; not counted as an unported healthy case |
| anchor: repaired Object/string serde decode failure | defensive decoder boundary; valid known/unknown/duplicate/deep/string cases proved | fixed `json:` anchor message, reported as an error, never healthy |
| audit metadata error except missing | reachable stat errors, e.g. file in ancestor | `stat <path>:` plus platform error text (approved deviation) |
| anchor read error except missing | reachable read error, e.g. directory | `anchor <path>: open <path>:` plus platform error text (approved deviation) |
| missing audit or anchor | healthy not-initialized/pending states, not delegation | existing native handling retained |
| discovery parse failure | reachable malformed client JSON/JSONC/YAML and typed entry errors | Go frame plus the native parser message (approved deviation) |
| discovery missing command and URL | reachable Go discovery error; multi-invalid map entries may affect first report | Go's message, first entry in source order |
| discovery file I/O error | reachable Go error report; missing-file classification is platform-dependent | existing admitted errors retained; wider exact raw-path/platform inventory remains open |
| unsupported discovery/config earlier in build_report | propagates above None before any native output | no remaining `None`; the whole report is still built before output |

The actual dispatcher reaches doctor, decide, version, grants and scan; libraries
named proxy/approval/sequence are not invented CLI cases. This inventory covers
doctor's explicit/propagated None boundaries, not every possible parser shape.
Broader scan malformed boundaries, stdout failure, full Brain command routing
and native acceptance remain separate unfinished #770/#769 contracts.
