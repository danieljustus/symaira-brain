package managed

import (
	_ "embed"
	"encoding/json"
	"fmt"
	"runtime"
	"strings"
)

//go:embed manifest.json
var defaultManifestJSON []byte

// Manifest defines the pinned versions for each managed core binary.
type Manifest struct {
	// SchemaVersion is the manifest format version (currently 1).
	SchemaVersion int `json:"schema_version"`
	// Cores maps binary name to its pinned version and GitHub source.
	Cores map[string]Core `json:"cores"`
}

// Core describes one managed core binary's pinned version and release source.
type Core struct {
	// Version is the pinned semver (e.g. "v0.15.3"). Release asset names
	// omit the leading "v", so asset-name construction normalizes it away.
	Version string `json:"version"`
	// Repo is the GitHub <owner>/<repo> for release downloads.
	Repo string `json:"repo"`
	// BinaryName is the executable name (e.g. "symvault").
	BinaryName string `json:"binary_name"`
	// AssetPrefix is the archive filename prefix (e.g. "symaira-vault").
	// The full asset name is constructed as:
	//   <AssetPrefix>_<version without v>_<os>_<arch>.tar.gz
	// Some releases omit the version from the asset name;
	// set AssetPrefix to the exact binary name in that case and the
	// download logic will try both patterns.
	AssetPrefix string `json:"asset_prefix"`
	// HasCosign indicates whether cosign signatures (.sig/.pem) are
	// published for this core's release archives.
	HasCosign bool `json:"has_cosign"`
	// SHA256 is the expected SHA-256 hash of the checksums.txt file
	// published alongside this core's release archive. Used by the
	// bump-core workflow to verify the checksums file before updating
	// the manifest entry.
	SHA256 string `json:"sha256"`
	// Platforms restricts which GOOS values this core ships for. Empty
	// means every platform. A core whose list does not include the
	// current GOOS is skipped silently — e.g. symcockpit is macOS-only
	// and must never be reported as a failed install elsewhere.
	Platforms []string `json:"platforms,omitempty"`
	// AssetArch overrides the architecture segment of the release asset
	// name. Some releases publish a single universal archive (e.g.
	// "universal") instead of per-arch archives.
	AssetArch string `json:"asset_arch,omitempty"`
}

// LoadManifest returns the embedded default manifest.
func LoadManifest() (*Manifest, error) {
	var m Manifest
	if err := json.Unmarshal(defaultManifestJSON, &m); err != nil {
		return nil, fmt.Errorf("managed: parse manifest: %w", err)
	}
	return &m, nil
}

// Platform returns the GOOS/GOARCH pair mapped to the release asset
// naming convention used by the Symaira repos. Returns an error for
// unsupported platforms.
func Platform() (goos, goarch string, err error) {
	goos = runtime.GOOS
	goarch = runtime.GOARCH
	switch goarch {
	case "arm64":
		// arm64 is the primary target; keep as-is
	case "amd64":
		// amd64 is the x86_64 fallback
	default:
		return "", "", fmt.Errorf("managed: unsupported architecture %q", goarch)
	}
	return goos, goarch, nil
}

// AssetName computes the release asset filename for a core on the
// current platform. The pinned version is normalized to drop a leading
// "v" (Symaira release assets carry no prefix even when tags do) and a
// core with an AssetArch override (e.g. "universal") uses that instead of
// the per-arch suffix.
func (c *Core) AssetName(goos, goarch string) string {
	return fmt.Sprintf("%s_%s_%s.tar.gz", c.AssetPrefix, stripV(c.Version), c.osArchSuffix(goos, goarch))
}

// AssetNameAlt returns the unversioned asset name fallback (used by
// releases that don't include the version in the filename.
func (c *Core) AssetNameAlt(goos, goarch string) string {
	return fmt.Sprintf("%s_%s.tar.gz", c.BinaryName, c.osArchSuffix(goos, goarch))
}

// ChecksumAssetName returns the checksum file asset name.
func (c *Core) ChecksumAssetName() string {
	// All repos publish a checksums.txt file
	return "checksums.txt"
}

// stripV normalizes a pinned version to the form used in release asset
// names: Symaira releases publish archives without a leading "v" even
// though their tags carry one.
func stripV(version string) string {
	return strings.TrimPrefix(version, "v")
}

// normalizeVersion strips a leading "v" (and surrounding whitespace) from
// a version string so comparisons between an installed binary's reported
// version and the manifest's pinned version work regardless of which form
// each side uses.
func normalizeVersion(version string) string {
	return strings.TrimPrefix(strings.TrimSpace(version), "v")
}

// SupportsPlatform reports whether the core ships for the given GOOS. An
// empty Platforms list means every platform.
func (c *Core) SupportsPlatform(goos string) bool {
	if len(c.Platforms) == 0 {
		return true
	}
	for _, p := range c.Platforms {
		if p == goos {
			return true
		}
	}
	return false
}

// osArchSuffix maps GOOS/GOARCH to the release archive convention. A core
// with an AssetArch override uses that value instead of the derived arch
// (e.g. a single "darwin_universal" archive for both arm64 and amd64).
func (c *Core) osArchSuffix(goos, goarch string) string {
	var os, arch string
	switch goos {
	case "darwin":
		os = "darwin"
	case "linux":
		os = "linux"
	case "windows":
		os = "windows"
	default:
		os = goos
	}
	switch goarch {
	case "arm64":
		arch = "arm64"
	case "amd64":
		arch = "amd64"
	default:
		arch = goarch
	}
	if c.AssetArch != "" {
		arch = c.AssetArch
	}
	return os + "_" + arch
}

// BinaryPath returns the path within an extracted tar.gz where the
// executable lives. Some repos wrap binaries in a directory; others
// put them at the root.
func (c *Core) BinaryPathInArchive(goos, goarch string) string {
	osArch := c.osArchSuffix(goos, goarch)
	// symaira-vault wraps binaries in a versioned directory named after
	// the archive (which carries the version without a "v" prefix).
	if strings.HasPrefix(c.AssetPrefix, "symaira-vault") {
		return fmt.Sprintf("%s_%s_%s/%s", c.AssetPrefix, stripV(c.Version), osArch, c.BinaryName)
	}
	// Non-vault archives put the binary at the archive root.
	return c.BinaryName
}
