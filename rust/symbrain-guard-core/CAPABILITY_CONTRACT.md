# Pure capability-token contract

This additive slice makes the Go capability signing and verification primitives
available in the Rust Guard kernel. The Go implementation remains the production
oracle and rollback path. There is no CLI, gateway, or issuance cutover.

The oracle revision is `0ccb0fe6c5afe668714d647eaa3497f8e6ed3f79`.
`guard/internal/capability/oracle_test.go` checks the current production files
against that Git object before generating or checking the fixture. The fixture
records production and generator SHA-256 digests separately. Its synthetic
master keys and claims contain no real credentials or user data.

| Contract | Executable evidence |
| --- | --- |
| HKDF-SHA256, nil salt, fixed purpose label, minimum 32-byte master | 5 derivation cases |
| Canonical claims JSON, HMAC-SHA256, unpadded URL base64 | 22 exact signing-byte cases |
| Key → decode → constant-time signature check → claims → expiry | 70 verification cases; exact error classification and text |
| Malformed input, null/default fields, repeated fields, Unicode, numeric bounds, raw base64 behavior | 37 exact decode/re-encode cases |
| Seven control-plane deny prefixes, exact scope, wildcard, empty target | 84 scope cases |
| Intersection with every identity-policy decision; existing denies retained | 42 ceiling cases |

There are 260 unique case IDs. Verified claims and ceiling results compare JSON
fields; signing, decode/re-encode, derivation, and scope results compare exact
Go-generated output bytes. No mismatch normalization is applied.

Go's `testing/synctest` freezes the production verifier clock at
`2000-01-01T00:00:00Z`; Rust receives the same Unix timestamp via `verify_at`.
Future `iat` values are accepted when positive and before `exp`, matching Go.
`exp == now` is expired. Nil and empty scopes both grant nothing but have distinct
signed JSON encodings. Duplicate ordinary scopes are permitted; duplicate
wildcards and empty entries are invalid claims.

`sign` is the pure signing primitive, corresponding to Go's package-private
helper. It does not choose or validate TTL, JTI, or claims. `Token::decode` also
does not authenticate. Callers must use `verify_at` before trusting claims,
then apply `in_scope` and the identity's policy ceiling. Like Go's ceiling,
`scope::scope_ceiling` alone does not enforce the control-plane deny list.

The four new crate dependencies are pinned: `base64` supplies the wire codec;
RustCrypto `hkdf`, `hmac`, and `sha2` supply domain-separated key derivation,
SHA-256, and constant-time MAC verification. The lockfile adds only their
cryptographic dependency graph. The key-storage slice additionally pins
`getrandom` for operating-system CSPRNG bytes. No cryptographic primitive is
implemented locally.

The key-storage adapter mirrors the Go persistence boundary: it resolves
`$XDG_DATA_HOME/symguard/capability.key` (or the home/temp fallback), loads
existing material unchanged, rejects present material shorter than 32 bytes,
and creates first-run material with owner-only `0600` permissions on Unix.
Random generation and persistence are still an adapter boundary; no CLI or
production path selects the Rust implementation.

## Reproduce

Use an isolated `CARGO_TARGET_DIR` and fresh `GOTMPDIR` appropriate to the checkout.
Keep the workspace manifest path absolute when invoking Cargo.

```sh
SYMBRAIN_CAPABILITY_UPDATE=1 go test ./guard/internal/capability -run '^TestCapabilityOracleFixture$' -count=1 -v
go test ./guard/internal/capability -count=1 -v
go test ./guard/internal/policy -run '^TestScopeCeiling' -count=1 -v
cargo test --manifest-path "$manifest" -p symbrain-guard-core -- --list
```

Run each of these discovered names with
`cargo test --manifest-path "$manifest" -p symbrain-guard-core --test capability_oracle_tests NAME -- --exact --nocapture`:

- `capability_oracle_provenance_and_case_ids_are_complete`
- `capability_signing_matches_pinned_go_bytes`
- `capability_verification_matches_pinned_go`
- `capability_wire_matches_pinned_go_bytes`
- `capability_scope_intersection_matches_pinned_go`

Each command executes one test. Both implementations emit
`CAPABILITY_CASE_ID=<id>` after executing each case. Compare the emitted sets to
the fixture's IDs and reject duplicates or zero cases. A normal Go test run
checks fixture bytes; it never updates expectations. The Go drift-rejection test
exercises both source and fixture byte comparators with changed output.

The initial macOS arm64 checkpoint passed all five capability tests, all five
existing Rust tests, 21 top-level Go capability tests plus 42 subtests, and two
Go ceiling tests plus eight subtests. The required rustfmt and Clippy gates
passed. An initial Clippy test-iterator lint was corrected before acceptance.

## Remaining contracts and gates

- Key loading, first-run generation and file persistence now have a Rust adapter
  with focused macOS tests; native Linux/Windows permission behavior and
  production caller integration remain pending. The Go implementation remains
  the issuance and rollback oracle.
- Purpose matching against a concrete operation, replay tracking, revocation,
  and CLI/gateway wiring are outside these pure primitives.
- The fixture harness calls Go and Rust separately in-process and has no
  interchangeable binary candidate interface; binary self-identity rejection
  is not supported. Source pinning and fixture drift rejection are exercised.
- Exhaustive parser fuzzing/property tests, native Linux/Windows runs, release
  gates, and performance/value benchmarks remain pending. This checkpoint is
  bounded fixture and key-storage parity, not full migration or cutover approval.
