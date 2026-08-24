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
	// Version is the pinned semver (e.g. "v0.15.3").
	Version string `json:"version"`
	// Repo is the GitHub <owner>/<repo> for release downloads.
	Repo string `json:"repo"`
	// BinaryName is the executable name (e.g. "symvault").
	BinaryName string `json:"binary_name"`
	// AssetPrefix is the archive filename prefix (e.g. "symaira-vault").
	// The full asset name is constructed as:
	//   <AssetPrefix>_<Version>_<os>_<arch>.tar.gz
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
// current platform. It tries the versioned pattern first
// (<prefix>_<version>_<os>_<arch>.tar.gz), then the unversioned
// pattern (<binary>_<os>_<arch>.tar.gz) for legacy releases.
func (c *Core) AssetName(goos, goarch string) string {
	osArch := osArchSuffix(goos, goarch)
	versioned := fmt.Sprintf("%s_%s_%s.tar.gz", c.AssetPrefix, c.Version, osArch)
	return versioned
}

// AssetNameAlt returns the unversioned asset name fallback (used by
// releases that don't include the version in the filename.
func (c *Core) AssetNameAlt(goos, goarch string) string {
	osArch := osArchSuffix(goos, goarch)
	return fmt.Sprintf("%s_%s.tar.gz", c.BinaryName, osArch)
}

// ChecksumAssetName returns the checksum file asset name.
func (c *Core) ChecksumAssetName() string {
	// All repos publish a checksums.txt file
	return "checksums.txt"
}

// osArchSuffix maps GOOS/GOARCH to the release archive convention.
func osArchSuffix(goos, goarch string) string {
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
	return os + "_" + arch
}

// BinaryPath returns the path within an extracted tar.gz where the
// executable lives. Some repos wrap binaries in a directory; others
// put them at the root.
func (c *Core) BinaryPathInArchive(goos, goarch string) string {
	osArch := osArchSuffix(goos, goarch)
	// symaira-vault wraps binaries in a versioned directory
	if strings.HasPrefix(c.AssetPrefix, "symaira-vault") {
		return fmt.Sprintf("%s_%s_%s/%s", c.AssetPrefix, c.Version, osArch, c.BinaryName)
	}
	// Non-vault archives put the binary at the archive root.
	return c.BinaryName
}
