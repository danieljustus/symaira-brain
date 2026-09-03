package managed

import (
	"bufio"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

// defaultBaseURL is the release host used when Installer.baseURL is unset.
const defaultBaseURL = "https://github.com"

// downloadURL constructs the release asset download URL against base
// (e.g. "https://github.com" or a test server URL), pinned to the
// given release tag. Earlier versions of this function used GitHub's
// "releases/latest/download/<asset>" alias, which silently drifts to
// whatever the repo's newest release is — that only matched the
// manifest's pinned Version by accident, and 404s on a versioned asset
// name as soon as the repo publishes a newer release. Pinning to the
// exact tag makes installs reproducible regardless of what the repo has
// released since.
func downloadURL(base, repo, tag, asset string) string {
	return fmt.Sprintf("%s/%s/releases/download/%s/%s", base, repo, tag, asset)
}

// downloadFile downloads a URL to the given path. It returns the number
// of bytes written and any error encountered.
func downloadFile(ctx context.Context, url, dest string) (int64, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return 0, fmt.Errorf("download: create request: %w", err)
	}
	req.Header.Set("Accept", "application/octet-stream")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return 0, fmt.Errorf("download: %s: %w", url, err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return 0, fmt.Errorf("download: %s: HTTP %d", url, resp.StatusCode)
	}

	f, err := os.Create(dest)
	if err != nil {
		return 0, fmt.Errorf("download: create %s: %w", dest, err)
	}
	defer func() {
		f.Close()
		// Clean up partial file on error
		if err != nil {
			os.Remove(dest)
		}
	}()

	n, err := io.Copy(f, resp.Body)
	if err != nil {
		return 0, fmt.Errorf("download: write %s: %w", dest, err)
	}
	return n, nil
}

// verifyChecksum checks a file against a SHA-256 checksum string.
func verifyChecksum(path, expected string) error {
	f, err := os.Open(path)
	if err != nil {
		return fmt.Errorf("checksum: open %s: %w", path, err)
	}
	defer f.Close()

	h := sha256.New()
	if _, err := io.Copy(h, f); err != nil {
		return fmt.Errorf("checksum: hash %s: %w", path, err)
	}
	actual := hex.EncodeToString(h.Sum(nil))
	if actual != strings.ToLower(strings.TrimSpace(expected)) {
		return fmt.Errorf("checksum: mismatch for %s: got %s, want %s", path, actual, expected)
	}
	return nil
}

// findChecksumInFile extracts the SHA-256 checksum for a given asset
// filename from a checksums.txt file. The file format is one line per
// asset: "<hex>  <filename>" or "<hex> *<filename>".
func findChecksumInFile(checksumsPath, assetName string) (string, error) {
	f, err := os.Open(checksumsPath)
	if err != nil {
		return "", fmt.Errorf("checksums: open: %w", err)
	}
	defer f.Close()

	scanner := bufio.NewScanner(f)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		// Format: "<hex>  <filename>" or "<hex> *<filename>"
		parts := strings.Fields(line)
		if len(parts) < 2 {
			continue
		}
		if parts[len(parts)-1] == assetName {
			return parts[0], nil
		}
	}
	return "", fmt.Errorf("checksums: asset %q not found in %s", assetName, checksumsPath)
}

// verifyCosign checks a cosign signature (.sig) and certificate (.pem)
// against an artifact, pinning the verification to core's exact GitHub
// Actions release-workflow identity and the GitHub OIDC issuer (see
// Core.CertificateIdentity / Core.CertificateOIDCIssuer) — it does not
// accept an artifact signed by any other identity.
//
// This fails closed by default: a missing cosign binary or missing
// signature/certificate files is an error, not a silent skip. Passing
// allowUnsigned=true downgrades that to a skip and prints a warning to
// warnOut — the explicit, opt-in escape hatch (--allow-unsigned) for
// environments that cannot install cosign or reach the signature
// assets. warnOut defaults to os.Stderr when nil.
func verifyCosign(ctx context.Context, artifactPath string, core *Core, allowUnsigned bool, warnOut io.Writer) error {
	if warnOut == nil {
		warnOut = os.Stderr
	}

	cosignPath, lookErr := exec.LookPath("cosign")

	sigPath := artifactPath + ".sig"
	pemPath := artifactPath + ".pem"
	_, sigErr := os.Stat(sigPath)
	_, pemErr := os.Stat(pemPath)

	var reason string
	switch {
	case lookErr != nil:
		reason = fmt.Sprintf("cosign is not installed on PATH (%v)", lookErr)
	case os.IsNotExist(sigErr):
		reason = fmt.Sprintf("signature file missing: %s", sigPath)
	case os.IsNotExist(pemErr):
		reason = fmt.Sprintf("certificate file missing: %s", pemPath)
	case sigErr != nil:
		reason = fmt.Sprintf("cannot read signature file %s: %v", sigPath, sigErr)
	case pemErr != nil:
		reason = fmt.Sprintf("cannot read certificate file %s: %v", pemPath, pemErr)
	}

	if reason != "" {
		if allowUnsigned {
			fmt.Fprintf(warnOut, "WARNING: skipping cosign verification for %s (--allow-unsigned): %s — the publisher was NOT authenticated\n",
				filepath.Base(artifactPath), reason)
			return nil
		}
		return fmt.Errorf("cannot verify publisher signature for %s: %s (pass --allow-unsigned to install anyway at your own risk)",
			filepath.Base(artifactPath), reason)
	}

	args := []string{"verify-blob", artifactPath,
		"--signature", sigPath,
		"--certificate", pemPath,
		"--certificate-identity", core.CertificateIdentity(),
		"--certificate-oidc-issuer", core.CertificateOIDCIssuer(),
	}

	cmd := exec.CommandContext(ctx, cosignPath, args...)
	output, err := cmd.CombinedOutput()
	if err != nil {
		return fmt.Errorf("cosign verify: %s: %w (output: %s)", artifactPath, err, string(output))
	}
	return nil
}
