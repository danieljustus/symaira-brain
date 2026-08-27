package managed

import (
	"archive/tar"
	"compress/gzip"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
)

// Installer handles downloading, verifying, and atomically installing
// managed core binaries into the managed directory.
type Installer struct {
	// BinDir is the target directory for installed binaries
	// (typically ~/.symaira/bin).
	BinDir string
	// TempDir is a scratch directory for downloads. If empty, the
	// system temp directory is used.
	TempDir string
	// baseURL is the release host to download from. If empty,
	// defaultBaseURL (github.com) is used. Tests override this to
	// point at an httptest.Server instead of the real network.
	baseURL string
}

// NewInstaller creates an Installer targeting the given binary directory.
func NewInstaller(binDir string) *Installer {
	return &Installer{BinDir: binDir}
}

// releaseBaseURL returns the configured base URL, or defaultBaseURL when unset.
func (inst *Installer) releaseBaseURL() string {
	if inst.baseURL == "" {
		return defaultBaseURL
	}
	return inst.baseURL
}

// Install downloads, verifies, and installs a single core binary.
// On verification failure the partially-installed binary is cleaned up,
// leaving no partial state behind (acceptance criterion: "verification
// failure aborts the install of that core with a clear error and leaves
// no partial binary behind").
func (inst *Installer) Install(ctx context.Context, core *Core) error {
	// Cores with a platform restriction (e.g. macOS-only symcockpit) are
	// skipped silently on other platforms — never a failed install.
	if !core.SupportsPlatform(runtime.GOOS) {
		return nil
	}

	goos, goarch, err := Platform()
	if err != nil {
		return err
	}

	// Determine the asset name, trying versioned then unversioned
	assetName := core.AssetName(goos, goarch)
	altName := core.AssetNameAlt(goos, goarch)

	// Create temp directory for downloads
	tmpDir := inst.TempDir
	if tmpDir == "" {
		tmpDir = os.TempDir()
	}
	dlDir, err := os.MkdirTemp(tmpDir, "symbrain-managed-*")
	if err != nil {
		return fmt.Errorf("managed: create temp dir: %w", err)
	}
	defer os.RemoveAll(dlDir)

	// Download the checksums file
	checksumsURL := downloadURL(inst.releaseBaseURL(), core.Repo, core.ChecksumAssetName())
	checksumsPath := filepath.Join(dlDir, "checksums.txt")
	if _, err := downloadFile(ctx, checksumsURL, checksumsPath); err != nil {
		return fmt.Errorf("managed: download checksums for %s: %w", core.BinaryName, err)
	}

	// Try downloading the versioned asset first
	archivePath, err := inst.downloadAndVerify(ctx, core, assetName, goos, goarch, checksumsPath, dlDir)
	if err != nil && assetName != altName {
		// Versioned asset not found; try unversioned
		archivePath, err = inst.downloadAndVerify(ctx, core, altName, goos, goarch, checksumsPath, dlDir)
	}
	if err != nil {
		return fmt.Errorf("managed: download %s: %w", core.BinaryName, err)
	}

	// Extract the binary
	binaryData, err := extractBinary(archivePath, core, goos, goarch)
	if err != nil {
		return fmt.Errorf("managed: extract %s: %w", core.BinaryName, err)
	}

	// Ensure bin directory exists
	if err := os.MkdirAll(inst.BinDir, 0o755); err != nil {
		return fmt.Errorf("managed: mkdir %s: %w", inst.BinDir, err)
	}

	// Atomic install: write to a temp file, then rename
	return atomicInstall(inst.BinDir, core.BinaryName, binaryData)
}

// downloadAndVerify downloads an asset, checks its checksum, and
// optionally verifies cosign. Returns the path to the downloaded file.
func (inst *Installer) downloadAndVerify(ctx context.Context, core *Core, assetName, goos, goarch, checksumsPath, dlDir string) (string, error) {
	url := downloadURL(inst.releaseBaseURL(), core.Repo, assetName)
	archivePath := filepath.Join(dlDir, assetName)
	if _, err := downloadFile(ctx, url, archivePath); err != nil {
		return "", err
	}

	checksum, err := findChecksumInFile(checksumsPath, assetName)
	if err != nil {
		return "", fmt.Errorf("checksum lookup: %w", err)
	}
	if err := verifyChecksum(archivePath, checksum); err != nil {
		return "", err
	}

	if core.HasCosign {
		if err := verifyCosign(ctx, archivePath); err != nil {
			return "", fmt.Errorf("cosign: %w", err)
		}
	}

	return archivePath, nil
}

// extractBinary reads a tar.gz archive and returns the binary's bytes.
func extractBinary(archivePath string, core *Core, goos, goarch string) ([]byte, error) {
	f, err := os.Open(archivePath)
	if err != nil {
		return nil, err
	}
	defer f.Close()

	gz, err := gzip.NewReader(f)
	if err != nil {
		return nil, fmt.Errorf("gzip: %w", err)
	}
	defer gz.Close()

	tr := tar.NewReader(gz)
	targetName := core.BinaryPathInArchive(goos, goarch)

	for {
		header, err := tr.Next()
		if err == io.EOF {
			break
		}
		if err != nil {
			return nil, fmt.Errorf("tar: %w", err)
		}

		name := header.Name
		if name == targetName || filepath.Base(name) == core.BinaryName {
			data, err := io.ReadAll(tr)
			if err != nil {
				return nil, fmt.Errorf("read binary from archive: %w", err)
			}
			return data, nil
		}
	}

	return nil, fmt.Errorf("binary %q not found in archive", core.BinaryName)
}

// atomicInstall writes binaryData to a temporary file, sets it
// executable, then atomically renames it to the final path.
func atomicInstall(binDir, binaryName string, data []byte) error {
	target := filepath.Join(binDir, binaryName)

	tmpFile, err := os.CreateTemp(binDir, ".install-*")
	if err != nil {
		return fmt.Errorf("atomic install: create temp: %w", err)
	}
	tmpPath := tmpFile.Name()

	if _, err := tmpFile.Write(data); err != nil {
		tmpFile.Close()
		os.Remove(tmpPath)
		return fmt.Errorf("atomic install: write: %w", err)
	}
	if err := tmpFile.Close(); err != nil {
		os.Remove(tmpPath)
		return fmt.Errorf("atomic install: close: %w", err)
	}
	if err := os.Chmod(tmpPath, 0o755); err != nil {
		os.Remove(tmpPath)
		return fmt.Errorf("atomic install: chmod: %w", err)
	}
	if err := os.Rename(tmpPath, target); err != nil {
		os.Remove(tmpPath)
		return fmt.Errorf("atomic install: rename: %w", err)
	}

	return nil
}

// InstalledVersion checks if a managed binary is installed and returns
// its version by running `<binary> version --json`.
func InstalledVersion(ctx context.Context, binDir, binaryName string) (string, error) {
	path := filepath.Join(binDir, binaryName)
	if _, err := os.Stat(path); os.IsNotExist(err) {
		return "", nil
	}

	ctx, cancel := context.WithTimeout(ctx, 3e9) // 3 seconds
	defer cancel()

	out, err := exec.CommandContext(ctx, path, "version", "--json").Output()
	if err != nil {
		return "", fmt.Errorf("probe %s: %w", binaryName, err)
	}

	var payload struct {
		Version string `json:"version"`
	}
	if err := json.Unmarshal(out, &payload); err != nil {
		return "", fmt.Errorf("parse %s version: %w", binaryName, err)
	}
	return payload.Version, nil
}
