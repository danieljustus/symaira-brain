# State-key bridge original baseline (implementation paused)

Original immutable registry Rust candidate:
`b363762cb85b879630fecd3fd141cba1ae85f71c`.
New isolated worktree `/workspace/symaira-daemon772-state-key`, branch
`issue/772-native-state-key-bridge`, remains clean at that head. The registry
review worktree and target remain unchanged. Parent requested baseline only
pending finalized independent registry findings; no bridge implementation edits.

Actual process report `/tmp/symaira-key-bridge-b363-baseline.json`, probe
`/tmp/symaira-key-bridge-baseline.py`, owned portable provider source
`/tmp/symaira-key-bridge-provider.go`, stdout
`/tmp/symaira-key-bridge-b363-baseline.log`. Binary, source-head, Go SDK,
manifest/fixture and probe hashes are retained in the report. The immutable
Go dcddcef0 source and six historical 652453d state fixtures remain pristine.
Disposable private HOME/XDG/PATH and synthetic fixture providers were used;
no operator vault/keychain or credentials are accessed. The key values are
public test fixture AB/CD, never user material.

17 paired actual startup cases: no/empty key, correct/wrong/trimmed environment
key, invalid environment key in text/JSON/YAML, eight vault return modes
(AB/CD, exit2/3/4, invalid, empty, timeout), and a regular file occupying the
state-store directory. Each successfully started daemon executes list,
metadata for all six Go plaintext/encrypted v1/v2/v3 files, and explicit clean.
All metadata inspections preserve the original file hashes and reveal no cookie
value. Explicit clean may remove expired owned copies; there is no SSOT copy.

Go: 10 successful starts / 7 actual startup failures / 80 recorded frames /
8 provider executions. Rust: 17 successful starts / 0 startup failures /
129 recorded frames / 0 provider executions.

Confirmed behavior gaps before fixing:

- With the correct key Go reads all six persisted versions. Rust still rejects
  encrypted v1/v2/v3 despite a valid configured environment key.
- Go resolves and caches the vault once at startup, before state-store creation;
  vault AB takes precedence over wrong environment CD. Exits2/3 and empty output
  legitimately fall back to environment; denied/invalid output fails startup.
  Rust never probes any provider, accepts every key configuration and starts.
- Invalid environment startup is Go exit1, text message on stderr or JSON/YAML
  internal-error envelope on stdout. Rust starts successfully for all formats.
- A blocking owned provider fails Go startup after **15.022 seconds**, exit1,
  `resolve state encryption key: context deadline exceeded`; Rust starts in
  about .021 seconds without consulting the provider.
- A file occupying the state directory fails Go startup before accepting IPC;
  Rust accepts IPC and only later fails the state operation.
- Stable state error context differs too: Go includes `decode state "name":`
  and `cipher: message authentication failed` for wrong keys. Missing-key v1/v2
  follow the legacy plaintext codec and produce JSON invalid-character errors;
  v3 explicitly requires a key provider. Rust currently has different raw
  codec errors and no named-state decode wrapper. Preserve these original
  observations; do not assume one no-key error for every historical version.

Implementation direction, unimplemented: build the existing Core Store with
material from the existing Core KeyResolver/SystemKeySources once per runtime
startup, retain its typed source and bounded provider lifecycle, and use the
same owned store for both state inspection and browser save/load. Reuse the
existing AES-GCM/AAD codec, zeroizing material and six historical fixtures.
Fail closed on denied/malformed keys and retain encrypted migration bytes.
Prepare legitimate native test fixtures for startup; do not weaken production
initialization to accommodate tests.

Darwin source-only fixture concern (not reproduced as native evidence here):
Go's keychainGet calls security via PATH and treats only exit44 as absent;
private PATH="" can therefore cause an actual Go startup error. A legitimate
owned security fixture returning44 must isolate native test startup from the
operator keychain. This baseline supports that on Darwin, but only Linux x86_64
was executed. Do not claim Darwin/Windows proof or waive any native CI gate.
