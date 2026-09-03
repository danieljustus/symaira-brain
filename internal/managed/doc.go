// Package managed manages the lifecycle of the external core binaries —
// symvault, symcockpit, and symdesk — in a dedicated runtime directory
// (~/.symaira/bin).
//
// It handles version pinning, download from GitHub releases, signature
// verification (cosign + SHA-256 checksums), and atomic installation.
// The managed directory is checked before PATH during binary resolution,
// giving managed binaries priority over Homebrew or other system installs.
//
// symdesk's manifest entry pins has_cosign: false because
// danieljustus/symaira-desktop does not yet publish cosign signatures
// for its release archives (tracked in
// danieljustus/symaira-desktop#799); until that ships, symdesk installs
// verify by SHA-256 checksum only, same as symcockpit today.
//
// The two main entry points are:
//   - [Setup]: download and install all pinned core versions.
//   - [Fix]: repair any missing or version-mismatched managed binaries.
package managed
