# Rust release pipeline

`.github/workflows/release.yml` publishes a release on every `v*` tag:

| Job | Output |
|---|---|
| `test` | `make rust-check` and the release-tooling unit tests on the tagged commit. |
| `build` (three native runners) | `symbrain_<version>_<os>_<arch>.tar.gz` (built from the root workspace) and `symbrowse_<version>_<os>_<arch>.tar.gz` (built from `browse/`, its own workspace and toolchain, `--locked --release`) for macOS arm64 and Linux amd64/arm64. Each archive contains only its product binary, `AGENTS.md`, `LICENSE` and `README.md` (no Go fallback) and is smoke-tested on its own platform. |
| `release` | Developer ID signed and notarized darwin binaries (`com.symaira.brain.cli`, `com.symaira.browse.cli`), `checksums.txt` (all six archives), the archive contract check, one CycloneDX SBOM per archive, a keyless cosign signature and certificate for every asset, the GitHub release (read back and re-verified) and the Homebrew formula (`symbrain` only) pushed to `danieljustus/homebrew-tap`. |
| `macos-gui-dmg`, `bump-homebrew` | Signed and notarized GUI DMG and the Homebrew cask. |

Linux builds run on Ubuntu 22.04, so they need glibc 2.35 or newer. Apple
cannot staple a notarization ticket to a bare command-line binary: Gatekeeper
checks it online instead.

`.github/workflows/rust-release.yml` builds the same six archives as an
unpublished, unsigned candidate on manual dispatch and on pull requests that
change the release tooling. It never creates a tag, release or tap update.

```sh
gh workflow run rust-release.yml --ref main   # unpublished candidate
```

## Tools

| Script | Purpose |
|---|---|
| `package.py archive` | Reproducible archive for one product (`--product symbrain`, the default, or `symbrowse`) and platform (same inputs + `SOURCE_DATE_EPOCH` → same bytes). |
| `package.py checksums` | `checksums.txt` in `sha256sum` format. |
| `package.py verify` | Release contract: exactly the six archives (both products on all three platforms), exact members and modes, documentation identical to the tree, matching checksums. |
| `smoke.py` | Native smoke test of one archive (`--product`): no Go fallback, `version`, `help` and native flag validation. |
| `manifests.py` | Homebrew formula (`Formula/symbrain.rb`) from a `checksums.txt`. |

## Verifying a release

```sh
identity=https://github.com/danieljustus/symaira-brain/.github/workflows/release.yml@refs/tags/v<version>
cosign verify-blob --certificate checksums.txt.pem --signature checksums.txt.sig \
  --certificate-identity "$identity" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com checksums.txt
shasum -a 256 -c checksums.txt
python3 scripts/rust-release/package.py verify --version <version> --dir <dir with archives and checksums.txt>
codesign --verify --strict --check-notarization -R '=anchor apple generic and certificate leaf[subject.OU] = "M4744F3TAA" and identifier "com.symaira.brain.cli"' symbrain
codesign --verify --strict --check-notarization -R '=anchor apple generic and certificate leaf[subject.OU] = "M4744F3TAA" and identifier "com.symaira.browse.cli"' symbrowse
```

`symbrowse` archives are verified the same way (cosign `.sig`/`.pem` next to
every archive and SBOM, `checksums.txt`). Rolling back means installing the
previous Rust release; after a release, the pinned `symbrowse` entry in
`rust/symbrain-managed/assets/manifest.json` is repinned from the published
checksums.

There is no rollback to the Go releases (v0.12.x and earlier): their assets
stay on their GitHub releases as history, but the tap is never pointed back at
them.
