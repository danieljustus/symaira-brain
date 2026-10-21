# Rust release candidate pipeline

`.github/workflows/rust-release.yml` builds the release archives from the Rust
CLI. It runs on manual dispatch and on pull requests that change it. It never
creates a tag, a GitHub release or a tap update: the published `v*` release
still comes from GoReleaser in `release.yml`. Making these archives the default
release is the cutover in #782.

## What a run produces

| Job | Output |
|---|---|
| `build` (six native runners) | `symbrain_<version>_<os>_<arch>.tar.gz` (`.zip` on Windows) for darwin, linux and windows on amd64 and arm64. Each archive contains `symbrain`, the `symbrain-go` fallback, `AGENTS.md`, `LICENSE` and `README.md`, and is smoke-tested on its own platform. |
| `bundle` | `checksums.txt`, the six-archive contract check, one CycloneDX SBOM per archive, and the Homebrew formula and Scoop manifest rendered from the checksums. |
| `sign` (only with `sign=true`) | Developer ID signed and notarized darwin binaries, repackaged archives, refreshed checksums, SBOMs and tap files, and a keyless cosign signature and certificate for every file. |

Candidate versions are `<workspace version>-candidate.<commit>`. Unsigned
archives are reproducible for the same commit. Linux builds run on Ubuntu 22.04, so they
need glibc 2.35 or newer. Apple cannot staple a notarization ticket to a bare
command-line binary: Gatekeeper checks it online instead.

```sh
gh workflow run rust-release.yml --ref main            # unsigned candidate
gh workflow run rust-release.yml --ref main -f sign=true
```

A signed run submits the darwin binaries to Apple and writes entries to the
public Rekor transparency log.

## Verifying a signed bundle

Download the `rust-release-signed-<version>` artifact, then:

```sh
identity=https://github.com/danieljustus/symaira-brain/.github/workflows/rust-release.yml@refs/heads/main
cosign verify-blob --certificate checksums.txt.pem --signature checksums.txt.sig \
  --certificate-identity "$identity" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com checksums.txt
shasum -a 256 -c checksums.txt
go run ./scripts/dist-oracle -hybrid-candidate-check -platforms all -version <version> -assets <dir with archives and checksums.txt>
codesign --verify --strict --check-notarization -R '=anchor apple generic and certificate leaf[subject.OU] = "M4744F3TAA" and identifier "com.symaira.brain.cli"' symbrain
```

The notary receipts (`notary-*-info.json`) must all report `Accepted`.

## Homebrew and Scoop

`scripts/rust-release/manifests.py` renders `symbrain.rb` (for
`danieljustus/homebrew-tap`, `Formula/`) and `symbrain.json` (for
`danieljustus/scoop-bucket`, repository root) from a `checksums.txt`. Both
install `symbrain` and `symbrain-go`, so the fallback is on `PATH`. They are
review artifacts only; the cutover publishes them.

## Rolling back to v0.12.x

The v0.12.x GoReleaser assets stay on their GitHub releases. To point the tap
and bucket back at them, render the Go-only layout from the old release's
checksums and publish the two files:

```sh
gh release download v0.12.0 --repo danieljustus/symaira-brain --pattern checksums.txt --dir rollback
python3 scripts/rust-release/manifests.py --version 0.12.0 --checksums rollback/checksums.txt --out rollback --legacy-go
```

For v0.12.0, `rollback/symbrain.rb` is byte-identical to the formula
GoReleaser published (`scripts/rust-release/test_manifests.py` checks this
against the recorded copy). After the tap and bucket are updated, users run
`brew update && brew reinstall symbrain`; Scoop users install or update
`symaira/symbrain` from the bucket.
