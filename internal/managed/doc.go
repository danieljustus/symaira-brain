// Package managed manages the lifecycle of the external vault core binary
// (symvault) in a dedicated runtime directory (~/.symaira/bin).
//
// It handles version pinning, download from GitHub releases, signature
// verification (cosign + SHA-256 checksums), and atomic installation.
// The managed directory is checked before PATH during binary resolution,
// giving managed binaries priority over Homebrew or other system installs.
//
// The two main entry points are:
//   - [Setup]: download and install all pinned core versions.
//   - [Fix]: repair any missing or version-mismatched managed binaries.
package managed
